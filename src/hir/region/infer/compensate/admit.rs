// audited: 2026-09-22
//! What funds a `tail` compensating release.
//!
//! The retain on the very node the release is keyed to, and the return frontier
//! an unfunded release must otherwise respect.
//!
//! docs/impl/region/mechanism.md

use super::super::*;
use crate::hir::region::Region;

/// The retains a `tail` per-arm decref may be funded by, indexed by the node
/// that raises the count, plus the return frontier the unfunded ones obey.
///
/// A `tail` per-arm decref is sound only at a node whose own instructions raise
/// the value's RC on the same path. The retain is what makes the release
/// provably non-zeroing: it releases the value's OWN owning reference — the
/// temp/arg reference the single `decref_point` frees on just one arm — and can
/// never drop a live value to zero. A node that merely READS the value (a
/// closure CALLED there, a pass-through whose result co-locates in the same
/// region) has no such guard and keeps the conservative single-`decref_point`
/// baseline.
///
/// Site-keyed, not mere membership: the retain and the decref must be the SAME
/// node, on one mutually-exclusive arm.
pub(super) struct TailFunding {
    /// The stored value of a `put`/`set`/`push`. The store raises the value's RC
    /// (compile-time `IncrefRegion`, unchecked: `cross_region_refs` source =
    /// value; or the runtime mutable-store funnel, checked: `funnel_store_sites`),
    /// so the live container reference keeps RC ≥ 1 after the per-arm decref.
    ///
    /// The byte-copy dual joins it: a `%string-push`/`%bytes-push` funnel copies
    /// the value's bytes and touches neither its incref nor its decref, so the
    /// wrapper's stranded `val` per-arm release is the value's TRUE last use —
    /// sound at the same sites (and `%del`'s in-body decref is excluded upstream,
    /// so no double-free reaches here).
    store_value_at_site: HashMap<HirId, std::collections::HashSet<Region>>,
    /// The `-mut` funnel's CONTAINER (arg0). Such a funnel returns its container
    /// pass-through, so a dispatch wrapper's mutable arm hands the container back
    /// and never releases the owned-param reference it holds — a leak on the
    /// container's own region, and by the outgoing-edge cascade on every heap
    /// member stored into it. The funnel's `pass_through_retain` raised the
    /// RETURNED value's RC, so a per-arm decref here releases only that stranded
    /// owned-param reference: the exact dual of the stored-value guard.
    container_at_site: HashMap<HirId, std::collections::HashSet<Region>>,
    /// The `Return` mint, and the one guard that admits a RETURN-ESCAPING region
    /// to the `tail` route: `lower_return` mints the caller's owning reference
    /// before the node's own releases, so a per-arm decref keyed here releases
    /// the callee's reference and leaves the caller's. This is the base case of a
    /// walk (`(if (= i 0) xs (go … xs))`), whose returning arm loses the
    /// `decref_point` max to the recursive arm's later use and is otherwise left
    /// with a mint and no release at all.
    return_mint_at_site: HashMap<HirId, std::collections::HashSet<Region>>,
    /// The regions a return hands to the caller, who frees them.
    tail_regions: rustc_hash::FxHashSet<Region>,
    /// The `-mut` funnel containers, exempt from the frontier: such a container
    /// is handed back pass-through yet holds a DISTINCT stranded owned-param
    /// reference no return covers.
    mut_container_regions: std::collections::HashSet<Region>,
}

impl TailFunding {
    pub(super) fn collect(
        info: &RegionInfo,
        escape: &crate::hir::EscapeInfo,
        return_sites: &[(HirId, Vec<Region>)],
    ) -> Self {
        // The escaping (returned) regions are the caller's to free; projected
        // from escape's authoritative return verdict, not a solver-local tail set.
        let tail_regions = super::super::escape::return_frontier_regions(
            escape,
            &info.alloc_region,
            &info.binding_source_regions,
        );
        let mut container_at_site: HashMap<HirId, std::collections::HashSet<Region>> =
            HashMap::new();
        let mut mut_container_regions: std::collections::HashSet<Region> =
            std::collections::HashSet::new();
        for (&site, containers) in &info.funnel_container_sites {
            container_at_site
                .entry(site)
                .or_default()
                .extend(containers.iter().copied());
            mut_container_regions.extend(containers.iter().copied());
        }
        let mut store_value_at_site: HashMap<HirId, std::collections::HashSet<Region>> =
            HashMap::new();
        for &(site, src, _) in &info.cross_region_refs {
            store_value_at_site.entry(site).or_default().insert(src);
        }
        for sites in [&info.funnel_store_sites, &info.funnel_bytecopy_value_sites] {
            for (&site, vals) in sites {
                store_value_at_site
                    .entry(site)
                    .or_default()
                    .extend(vals.iter().copied());
            }
        }
        let mut return_mint_at_site: HashMap<HirId, std::collections::HashSet<Region>> =
            HashMap::new();
        for (site, regions) in return_sites {
            return_mint_at_site
                .entry(*site)
                .or_default()
                .extend(regions.iter().copied());
        }
        TailFunding {
            store_value_at_site,
            container_at_site,
            return_mint_at_site,
            tail_regions,
            mut_container_regions,
        }
    }

    /// Is a `tail` release of `r` at `node` funded by a retain on that same node?
    ///
    /// The return mint doubles as the per-path return-frontier admission — it IS
    /// the hand-over — while the store and container retains say nothing about
    /// whether this arm also returns the value, so they keep the frontier
    /// exclusion (docs/impl/region/mechanism.md § "The return frontier is
    /// per-path").
    ///
    /// The container case: `node` is a monomorphic funnel whose container (arg0)
    /// is `r`. A `-mut` funnel returns the container pass-through; an immutable
    /// funnel returns a fresh copy, so the container is genuinely dead in the arm.
    pub(super) fn admits(&self, r: Region, node: HirId) -> bool {
        if self.at(&self.return_mint_at_site, r, node) {
            return true;
        }
        if self.tail_regions.contains(&r) && !self.mut_container_regions.contains(&r) {
            return false;
        }
        self.at(&self.store_value_at_site, r, node) || self.at(&self.container_at_site, r, node)
    }

    /// Is `r` the container arg0 of the funnel at `node`?
    pub(super) fn container_at(&self, r: Region, node: HirId) -> bool {
        self.at(&self.container_at_site, r, node)
    }

    fn at(
        &self,
        m: &HashMap<HirId, std::collections::HashSet<Region>>,
        r: Region,
        node: HirId,
    ) -> bool {
        m.get(&node).is_some_and(|s| s.contains(&r))
    }
}
