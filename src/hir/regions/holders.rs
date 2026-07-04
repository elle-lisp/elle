//! Region → holder-binding index: the shared `sole_held` / alias model.
//!
//! A *holder* of a region `r` is a user binding whose value may point into `r`
//! (`RegionInfo::binding_source_regions`, plus any reassign-site value regions a
//! consumer folds in with [`RegionHolders::add`]). One index backs the consumers
//! that previously each rebuilt their own ad-hoc map: the reassign
//! 1-slot-container gate (`regions::analyze`) and the builder-idiom merge seed
//! (`regions::merge`) — and, as the ownership inference lands, its sole-held check.
//!
//! Two rules are baked into the type so no consumer can drop them:
//!
//! - **Synthetic temps are never holders.** A synthetic ANF producer temp
//!   (`SymbolId::SYNTHETIC` — the `(let [_t e] _t)` read-once-and-flow-on binding)
//!   aliases nothing a consumer reasons about, so it is excluded universally, at
//!   construction and in [`add`] alike. Each consumer layers its own *eligibility*
//!   predicate on top (the reassign gate additionally requires the binding to be
//!   read; the merge seed does not), because the genuinely shared core is the index
//!   and its queries, not the per-consumer filter.
//! - **Holders are a set, so a region is counted once per distinct binding.** The
//!   underlying `binding_source_regions[b]` is duplicate-free by construction —
//!   every `binding_regions` write in `regions::walk{,rest}` either stores a
//!   `dedup_regions`'d walk result or unions with `!contains` — so set semantics
//!   lose nothing, and they make "more than one holder" mean "aliased by two
//!   *distinct* bindings", which is exactly what the merge seed's `len() > 1` alias
//!   test and `sole_held` both ask.

use super::*;
use rustc_hash::FxHashSet;

/// Region → the distinct user bindings that may hold a value in it.
pub(super) struct RegionHolders {
    map: HashMap<Region, FxHashSet<Binding>>,
}

impl RegionHolders {
    /// Build the index from a `binding → source regions` map (the solver's
    /// `binding_source_regions`), keeping a binding iff it is a non-synthetic user
    /// binding for which `eligible(b)` holds. Fold additional holders (reassign-site
    /// value regions, which are not in `binding_source_regions`) in afterward with
    /// [`add`].
    pub(super) fn from_source_regions(
        source_regions: &HashMap<Binding, Vec<Region>>,
        arena: &BindingArena,
        mut eligible: impl FnMut(Binding) -> bool,
    ) -> Self {
        let mut holders = RegionHolders {
            map: HashMap::new(),
        };
        for (&b, regions) in source_regions {
            if is_user_binding(b, arena) && eligible(b) {
                holders.insert(b, regions);
            }
        }
        holders
    }

    /// Record that binding `b` holds each of `regions`. The caller has already
    /// applied its own eligibility filter; the universal synthetic exclusion is
    /// re-checked here so no fold-in path can smuggle a temp into the index.
    pub(super) fn add(&mut self, b: Binding, arena: &BindingArena, regions: &[Region]) {
        if is_user_binding(b, arena) {
            self.insert(b, regions);
        }
    }

    /// The distinct holders of `r`, or `None` when `r` has none. The merge seed
    /// reads this to refuse an aliased child (`len() > 1`) and to check the lone
    /// holder's escape facets.
    pub(super) fn holders_of(&self, r: Region) -> Option<&FxHashSet<Binding>> {
        self.map.get(&r)
    }

    /// `r` is *sole-held by `b`*: no holder other than `b`. A region with no
    /// recorded holder is sole-held by anything (nothing aliases it).
    pub(super) fn sole_held(&self, b: Binding, r: Region) -> bool {
        self.map.get(&r).is_none_or(|hs| hs.iter().all(|&h| h == b))
    }

    fn insert(&mut self, b: Binding, regions: &[Region]) {
        for &r in regions {
            self.map.entry(r).or_default().insert(b);
        }
    }
}

/// A user binding is anything that is not the synthetic ANF producer temp — the
/// one exclusion both consumers make unconditionally.
fn is_user_binding(b: Binding, arena: &BindingArena) -> bool {
    arena.get(b).name != crate::value::SymbolId::SYNTHETIC
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::arena::BindingScope;
    use crate::value::SymbolId;

    /// A non-synthetic user binding (any real symbol name).
    fn user(arena: &mut BindingArena) -> Binding {
        arena.alloc(SymbolId(1), BindingScope::Local)
    }

    /// The universal exclusion: a synthetic ANF temp holding a region is not a
    /// holder, so a region it shares with a user binding stays sole-held by the
    /// user binding. (Counterfactual: were synthetics admitted, `holders_of` would
    /// report two holders and `sole_held` would be false.)
    #[test]
    fn synthetic_temps_are_never_holders() {
        let mut arena = BindingArena::new();
        let u = user(&mut arena);
        let t = arena.gensym(); // name == SymbolId::SYNTHETIC
        let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
        src.insert(u, vec![Region(2)]);
        src.insert(t, vec![Region(2)]);

        let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

        let hs = holders.holders_of(Region(2)).expect("user holder recorded");
        assert_eq!(hs.len(), 1, "synthetic temp excluded from the holder set");
        assert!(hs.contains(&u));
        assert!(
            holders.sole_held(u, Region(2)),
            "a synthetic alias does not break sole-held"
        );
    }

    /// The per-consumer eligibility predicate filters on top of the synthetic
    /// exclusion (the reassign gate's "is read"). A user binding the predicate
    /// rejects is not a holder.
    #[test]
    fn eligibility_predicate_excludes_rejected_bindings() {
        let mut arena = BindingArena::new();
        let kept = user(&mut arena);
        let dropped = user(&mut arena);
        let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
        src.insert(kept, vec![Region(2)]);
        src.insert(dropped, vec![Region(2)]);

        let holders = RegionHolders::from_source_regions(&src, &arena, |b| b == kept);

        let hs = holders.holders_of(Region(2)).expect("kept holder recorded");
        assert_eq!(hs.len(), 1);
        assert!(hs.contains(&kept));
        assert!(!hs.contains(&dropped), "ineligible binding excluded");
    }

    /// Two distinct user bindings holding one region make it aliased: `holders_of`
    /// reports both and neither sole-holds it. This is the merge seed's refusal
    /// condition (`len() > 1`).
    #[test]
    fn two_distinct_user_holders_alias_the_region() {
        let mut arena = BindingArena::new();
        let a = user(&mut arena);
        let b = user(&mut arena);
        let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
        src.insert(a, vec![Region(2)]);
        src.insert(b, vec![Region(2)]);

        let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

        assert_eq!(holders.holders_of(Region(2)).map(|hs| hs.len()), Some(2));
        assert!(!holders.sole_held(a, Region(2)));
        assert!(!holders.sole_held(b, Region(2)));
    }

    /// A region with no recorded holder is sole-held by anything, and `holders_of`
    /// is `None` — the merge seed treats a holderless child as non-escaping, the
    /// reassign gate treats it as sole-held.
    #[test]
    fn unheld_region_is_sole_held_by_anything() {
        let mut arena = BindingArena::new();
        let a = user(&mut arena);
        let holders = RegionHolders::from_source_regions(&HashMap::new(), &arena, |_| true);

        assert!(holders.holders_of(Region(9)).is_none());
        assert!(holders.sole_held(a, Region(9)));
    }

    /// `add` folds reassign-site regions into an index already built from
    /// `binding_source_regions`, unioning per region — and re-applies the synthetic
    /// exclusion so a fold-in path cannot smuggle a temp in.
    #[test]
    fn add_folds_in_extra_holders_and_excludes_synthetics() {
        let mut arena = BindingArena::new();
        let base = user(&mut arena);
        let extra = user(&mut arena);
        let temp = arena.gensym();
        let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
        src.insert(base, vec![Region(2)]);

        let mut holders = RegionHolders::from_source_regions(&src, &arena, |_| true);
        holders.add(extra, &arena, &[Region(2)]);
        holders.add(temp, &arena, &[Region(2)]); // excluded — synthetic

        let hs = holders.holders_of(Region(2)).expect("holders recorded");
        assert_eq!(hs.len(), 2, "base + extra, temp excluded");
        assert!(hs.contains(&base) && hs.contains(&extra));
        assert!(!hs.contains(&temp));
    }

    /// A duplicate region in a binding's source list does not inflate the holder
    /// count — the set keeps one entry per distinct binding. This pins the
    /// invariant that lets the merge seed read `len() > 1` as a *distinct*-binding
    /// alias test even though it ports from a `Vec`-based map.
    #[test]
    fn duplicate_source_region_does_not_inflate_holder_count() {
        let mut arena = BindingArena::new();
        let b = user(&mut arena);
        let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
        src.insert(b, vec![Region(2), Region(2)]);

        let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

        assert_eq!(holders.holders_of(Region(2)).map(|hs| hs.len()), Some(1));
        assert!(holders.sole_held(b, Region(2)));
    }
}
