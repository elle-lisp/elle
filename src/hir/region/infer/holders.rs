// audited: 2026-11-13
//! Region → holder-binding index: the shared `sole_held` / alias model.
//!
//! docs/impl/region/bindings.md
//! docs/impl/anf.md
//!
//! A *holder* of a region `r` is a binding whose value may point into `r`
//! (`RegionInfo::binding_source_regions`, plus any reassign-site value regions a
//! consumer folds in with [`RegionHolders::add`]). One index backs every consumer
//! of the question: the reassign 1-slot-container gate (`region::infer::analyze`),
//! the builder-idiom merge seed and the closure-cycle merge (`region::infer::merge`),
//! and the ownership walks' sole-held check (`region::infer::ownership`).
//!
//! Two rules are built into the type so no consumer can drop them:
//!
//! - **An ANF temp is never a holder.** The ANF lift names a value with
//!   `(let [_t e] _t)`, whose body reads the temp once and hands its one
//!   reference on. The lift marks each temp it mints (`BindingInner::is_anf_temp`),
//!   and the index reads that mark, never a name. Every other binding is a
//!   holder, a binding another pass makes included: a later node may read it
//!   many times, as a loop reads its hoisted constants on every pass. The index
//!   applies the exclusion at construction and in `add` alike. Each consumer
//!   layers its own *eligibility* predicate on top: the reassign gate requires
//!   the binding to be read and not to be a store's feeder, and the merges and
//!   ownership walks require neither.
//! - **Holders are a set, so a region is counted once per distinct binding**,
//!   however many times a binding's source list names it. "More than one
//!   holder" therefore means "aliased by two *distinct* bindings", which is what
//!   the merge seed's `len() > 1` alias test and `sole_held` both ask.
//!
//! Counting *bindings* is a proxy for counting *references*, and the two diverge
//! wherever the canonical IR gives one source name two bindings. A consumer that
//! can name such a pair, and argue the pair holds one reference between them,
//! supplies it as an **alias map** to [`RegionHolders::with_aliases`]. The index
//! folds the forwarded-from binding's holdings onto the binding that carries them
//! forward before any query. The argument that justifies an entry is the
//! consumer's. The index only guarantees that the fold applies on every insert
//! path and on the query side: an alias entry makes the two bindings
//! indistinguishable to [`RegionHolders::sole_held`], so either name asks about
//! the one holding. A consumer whose bindings form a *chain* resolves it to its
//! last member before handing the map over, because the index reads one entry,
//! never a sequence of them.

use super::*;
use rustc_hash::FxHashSet;

/// Region → the distinct bindings that may hold a value in it.
pub(super) struct RegionHolders {
    map: HashMap<Region, FxHashSet<Binding>>,
    /// `forwarded-from → carries-forward`: bindings the consumer has shown to be
    /// one name holding one reference, folded onto the second at insert time.
    aliases: HashMap<Binding, Binding>,
}

impl RegionHolders {
    /// Build the index from a `binding → source regions` map (the solver's
    /// `binding_source_regions`), keeping each binding that is no ANF temp and for
    /// which `eligible(b)` holds. Fold additional holders (reassign-site value
    /// regions, which are not in `binding_source_regions`) in afterward with `add`.
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
            if may_hold(b, arena) && eligible(b) {
                holders.insert(b, regions.iter().copied());
            }
        }
        holders
    }

    /// Record that binding `b` holds each of `regions`. The caller has already
    /// applied its own eligibility filter. The index checks the ANF-temp exclusion
    /// again here, so no fold-in path can smuggle a temp into the index.
    pub(super) fn add(
        &mut self,
        b: Binding,
        arena: &BindingArena,
        regions: impl IntoIterator<Item = Region>,
    ) {
        if may_hold(b, arena) {
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
    ///
    /// The query resolves through the alias map exactly as an insert does. An
    /// alias entry says the two bindings hold one reference between them, so
    /// each of them must be able to ask about it — otherwise the folded-away
    /// name reads its own holding as somebody else's.
    pub(super) fn sole_held(&self, b: Binding, r: Region) -> bool {
        let b = self.resolve(b);
        self.map.get(&r).is_none_or(|hs| hs.iter().all(|&h| h == b))
    }

    /// The binding an alias entry folds `b` onto, or `b` itself. Resolved ONE
    /// step, not transitively: an entry asserts that this pair holds a single
    /// reference, and chaining two such assertions is a different claim. The
    /// consumer states that claim by mapping every member of its chain to the
    /// same binding (see `with_aliases`).
    fn resolve(&self, b: Binding) -> Binding {
        self.aliases.get(&b).copied().unwrap_or(b)
    }

    fn insert(&mut self, b: Binding, regions: impl IntoIterator<Item = Region>) {
        let b = self.resolve(b);
        for r in regions {
            self.map.entry(r).or_default().insert(b);
        }
    }
}

/// Every binding but an ANF temp may hold a value: the one exclusion every
/// consumer makes.
fn may_hold(b: Binding, arena: &BindingArena) -> bool {
    !arena.get(b).is_anf_temp()
}

#[cfg(test)]
mod tests;
