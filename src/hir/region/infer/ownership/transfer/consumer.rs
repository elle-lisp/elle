// audited: 2026-10-21
//! The consumer gate the call and fiber faces read at each site that receives a producer's result.
//!
//! docs/impl/region/owner.md

use super::*;

impl Cut<'_> {
    /// The result region of consumer `site`, when it is adoptable: it crosses no
    /// frontier, appears in no edge, belongs to no dynamic class, and is
    /// discard-shaped — every binding the result flows into passes
    /// [`Cut::reads_pass`].
    ///
    /// The result flow is followed BOTH through the site's own placeholder region
    /// and through the producer's ROOT region, so a binding that names the root
    /// directly is gated too. A holder bound inside the producer is the
    /// producer's own binding, whose reads precede the hand-off. A holder bound
    /// outside is a consumer-side read and must pass.
    pub(super) fn consumer_result(
        &self,
        site: HirId,
        root: Region,
        producer: Interval,
    ) -> Option<Region> {
        let info = self.info;
        let &r = info.alloc_region.get(&site)?;
        if self.inputs.shared().contains(&r)
            || self.in_any_edge(r)
            || info.cell_release_regions.contains(&r)
            || info.suppressed_decref_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || self.is_merged(r)
        {
            return None;
        }
        // The bindings bound directly (or through the ANF wrapper) to this call's
        // value.
        if let Some(holders) = self.ix.bound_to.get(&site) {
            if !holders.iter().all(|&b| self.reads_pass(b)) {
                return None;
            }
        }
        // Every binding the result-flow regions reach, outside the producer.
        for (&b, regions) in &info.binding_source_regions {
            if !(regions.contains(&r) || regions.contains(&root)) {
                continue;
            }
            if self.bound_inside(b, producer) {
                continue;
            }
            if !self.reads_pass(b) {
                return None;
            }
        }
        Some(r)
    }
}
