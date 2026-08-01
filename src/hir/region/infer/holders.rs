//! Region → holder-binding index: the shared `sole_held` / alias model.
//!
//! A *holder* of a region `r` is a user binding whose value may point into `r`
//! (`RegionInfo::binding_source_regions`, plus any reassign-site value regions a
//! consumer folds in with [`RegionHolders::add`]). One index backs the consumers
//! that previously each rebuilt their own ad-hoc map: the reassign
//! 1-slot-container gate (`region::infer::analyze`) and the builder-idiom merge seed
//! (`region::infer::merge`) — and, as the ownership inference lands, its sole-held check.
//!
//! Two rules are baked into the type so no consumer can drop them:
//!
//! - **Synthetic temps are never holders.** A synthetic ANF producer temp
//!   (`SymbolId::SYNTHETIC` — the `(let [_t e] _t)` read-once-and-flow-on binding)
//!   aliases nothing a consumer reasons about, so it is excluded universally, at
//!   construction and in `add` alike. Each consumer layers its own *eligibility*
//!   predicate on top (the reassign gate additionally requires the binding to be
//!   read; the merge seed does not), because the genuinely shared core is the index
//!   and its queries, not the per-consumer filter.
//! - **Holders are a set, so a region is counted once per distinct binding.** The
//!   underlying `binding_source_regions[b]` is duplicate-free by construction —
//!   every `binding_regions` write in `region::infer::walk{,rest}` either stores a
//!   `dedup_regions`'d walk result or unions with `!contains` — so set semantics
//!   lose nothing, and they make "more than one holder" mean "aliased by two
//!   *distinct* bindings", which is exactly what the merge seed's `len() > 1` alias
//!   test and `sole_held` both ask.
//!
//! Counting *bindings* is a proxy for counting *references*, and the two diverge
//! wherever the canonical IR gives one source name two bindings. A consumer that
//! can name such a pair — and argue the pair holds one reference between them —
//! supplies it as an **alias map** to [`RegionHolders::with_aliases`], which folds
//! the forwarded-from binding's holdings onto the binding that carries them
//! forward before the index is queried. The argument that justifies an entry is
//! the consumer's; the index only guarantees the fold is applied on every insert
//! path.

use super::*;
use rustc_hash::FxHashSet;

/// Region → the distinct user bindings that may hold a value in it.
pub(super) struct RegionHolders {
    map: HashMap<Region, FxHashSet<Binding>>,
    /// `forwarded-from → carries-forward`: bindings the consumer has shown to be
    /// one name holding one reference, folded onto the second at insert time.
    aliases: HashMap<Binding, Binding>,
}

impl RegionHolders {
    /// Build the index from a `binding → source regions` map (the solver's
    /// `binding_source_regions`), keeping a binding iff it is a non-synthetic user
    /// binding for which `eligible(b)` holds. Fold additional holders (reassign-site
    /// value regions, which are not in `binding_source_regions`) in afterward with
    /// `add`.
    pub(super) fn from_source_regions(
        source_regions: &HashMap<Binding, Vec<Region>>,
        arena: &BindingArena,
        eligible: impl FnMut(Binding) -> bool,
    ) -> Self {
        Self::with_aliases(source_regions, arena, eligible, HashMap::new())
    }

    /// `from_source_regions`, resolving each holder through `aliases` first — see
    /// the module header. An alias entry makes the two bindings indistinguishable
    /// to every query, so a consumer must own the argument that they hold one
    /// reference between them.
    pub(super) fn with_aliases(
        source_regions: &HashMap<Binding, Vec<Region>>,
        arena: &BindingArena,
        mut eligible: impl FnMut(Binding) -> bool,
        aliases: HashMap<Binding, Binding>,
    ) -> Self {
        let mut holders = RegionHolders {
            map: HashMap::new(),
            aliases,
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
        // Resolved ONE step, not transitively: an alias entry asserts that this
        // pair holds a single reference, and chaining two such assertions is a
        // different claim the consumer has not made (see `with_aliases`).
        let b = self.aliases.get(&b).copied().unwrap_or(b);
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
mod tests;
