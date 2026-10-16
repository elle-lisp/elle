// audited: 2026-10-14
//! The alias obligation: the values a subtree's drop can tear whose release no member's own point names.
//!
//! docs/impl/region/adopt.md

use super::super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// The alias relations the lifetime obligation closes over, indexed by the
/// region each alias is reached from. Built once per ownership pass; the
/// per-root closure walks only the edges out of the regions it reaches.
///
/// Reachability and boundedness are tracked separately. A region can be
/// reachable without needing a bound (a funnel result), and one relation must
/// not consume the other's obligation: a read alias that a funnel edge happened
/// to make reachable first still owes its own bound.
pub(super) struct AliasTables {
    /// `source → aliases` that owe a bound: a native container read's result
    /// (`counted_read_aliases`), an opaque call's result
    /// (`opaque_result_aliases`), and the region a value-routed release reads
    /// through a compiled capture cell (`cell_route_regions`).
    bounded: FxHashMap<Region, Vec<Region>>,
    /// `container → funnel results` (`funnel_result_containers`): reachable,
    /// and owed no bound of their own. A `Funnel`'s result is arg0 in place or
    /// a fresh copy of it, the container and never an interior element, so it
    /// carries arg0's own counted reference where arg0 is the root and no-ops on
    /// the frozen region where arg0 is a member. Bounding it would refuse every
    /// mutable-store subtree over its own trailing discarded store result.
    funnel: FxHashMap<Region, Vec<Region>>,
}

impl AliasTables {
    pub(super) fn new(info: &RegionInfo) -> Self {
        let mut bounded: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
        for &(_, alias, source) in info
            .counted_read_aliases
            .iter()
            .chain(info.opaque_result_aliases.iter())
        {
            bounded.entry(source).or_default().push(alias);
        }
        // A release routed through a compiled capture cell loads the cell and
        // unwraps it, so it reads the cell's page: the routed region is an alias
        // of the cell, reachable where the cell is and owed the same bound.
        for (&cell, &routed) in &info.cell_route_regions {
            bounded.entry(cell).or_default().push(routed);
        }
        let mut funnel: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
        for &(_, result, container) in &info.funnel_result_containers {
            funnel.entry(container).or_default().push(result);
        }
        AliasTables { bounded, funnel }
    }

    /// The release point of every alias reachable from `members`, closed to a
    /// fixpoint. `None` stands for an alias with no recorded release, which
    /// bounds nothing and so refuses, exactly as an unbounded member does.
    ///
    /// A native container read hands the reader a value that still lives inside
    /// the container, under a counted pass-through retain the RC baseline
    /// honours. Adoption freezes the member's RC, so that retain buys nothing
    /// and the root's drop reclaims the element under the reader, whose own
    /// value-resolved release then faults on the freed page. The alias's region
    /// is a fresh call-result placeholder no containment edge relates to any
    /// member, so the member obligation never sees it.
    ///
    /// Aliasing is transitive: `(get (get c 0) 0)` names a value inside a
    /// member of a member, and the inner alias's bound says nothing about the
    /// outer one's. The closure is bounded by the alias count, since each round
    /// adds at least one region. An opaque call's result may BE an argument or
    /// a value read out of one, so reaching it bounds its own release and lets
    /// the read edges reach on through it.
    pub(super) fn release_points(
        &self,
        members: &FxHashSet<Region>,
        info: &RegionInfo,
    ) -> Vec<Option<HirId>> {
        let mut reachable: FxHashSet<Region> = members.clone();
        let mut bounded: FxHashSet<Region> = FxHashSet::default();
        let mut out: Vec<Option<HirId>> = Vec::new();
        let mut worklist: Vec<Region> = reachable.iter().copied().collect();
        while let Some(src) = worklist.pop() {
            if let Some(aliases) = self.bounded.get(&src) {
                for &alias in aliases {
                    // Bounding is per edge, not per new reach: an alias a funnel
                    // edge made reachable first still owes its bound.
                    if bounded.insert(alias) {
                        out.push(info.region_data.get(&alias).map(|d| d.lifetime_point));
                    }
                    if reachable.insert(alias) {
                        worklist.push(alias);
                    }
                }
            }
            if let Some(results) = self.funnel.get(&src) {
                for &result in results {
                    if reachable.insert(result) {
                        worklist.push(result);
                    }
                }
            }
        }
        out
    }
}
