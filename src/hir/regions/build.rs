use super::*;

impl RegionInference {
    /// Build the final RegionInfo from the walk's direct outputs.
    /// Every allocation already has its unique region from the walk, so this
    /// assembles `RegionInfo` directly without a constraint-solving pass.
    /// `cross_region_refs` was recorded by the walk at the moment each
    /// storage / capture / opaque-call edge appeared.
    pub(super) fn build_info(self) -> RegionInfo {
        use rustc_hash::FxHashSet;

        // Every allocation HirId has a region from the walk.
        for (hir_id, region) in &self.alloc_region {
            assert!(
                region.0 != 0,
                "allocation @{} resolved to Region(0) — synthetic root should prevent this",
                hir_id.0
            );
        }

        // `live_regions` is the set of regions that hold allocations. Each
        // allocation has its own leaf region; scope_regions don't directly hold
        // allocs, so `live_regions` is the transitive union: alloc regions plus
        // their ancestor scope_regions in the tree. `scope_has_local_allocs`
        // reads it to ask whether a scope holds any local allocation.
        let scope_regions: FxHashSet<Region> = self.scope_region.values().copied().collect();
        let mut live_regions: FxHashSet<Region> = FxHashSet::default();
        // Pre-allocated capture cells are real allocations too (one
        // MakeCaptureCell per entry) — they just aren't in `alloc_region`
        // (which is HirId-keyed; N cells share one Begin HirId).
        let cell_rs = self
            .begin_cell_regions
            .values()
            .flat_map(|v| v.iter().map(|(_, r)| *r));
        for alloc_r in self.alloc_region.values().copied().chain(cell_rs) {
            live_regions.insert(alloc_r);
            let mut cur = Some(alloc_r);
            while let Some(r) = cur {
                if scope_regions.contains(&r) {
                    live_regions.insert(r);
                }
                cur = self.tree.parent_of(r);
            }
        }

        let live_count = self
            .scope_region
            .values()
            .filter(|r| live_regions.contains(r))
            .count();
        let empty_count = self
            .scope_region
            .values()
            .filter(|r| !live_regions.contains(r))
            .count();

        let stats = RegionStats {
            regions_created: self.next_region as usize,
            constraints_generated: 0,
            solver_iterations: 0,
            live_scopes: live_count,
            empty_scopes: empty_count,
        };

        // Filter cross_region_refs to only those whose source region
        // is "live" (i.e. corresponds to an actual allocation).
        // Cross-region refs from scope regions (binding_region for a
        // non-allocating binding, etc.) would otherwise leak in.
        let cross_region_refs: Vec<(HirId, Region, Region)> = self
            .cross_region_refs
            .into_iter()
            .filter(|(_, src, _)| live_regions.contains(src))
            .collect();

        // Funnel-store containment recovered from the container's RetType. Filter
        // to live source regions, mirroring `cross_region_refs`: a contained value
        // that resolves to a phantom (a borrowed param, not in `live_regions`) owns
        // no allocation, so it is not a subtree member.
        let containment_edges: Vec<(HirId, Region, Region)> = self
            .containment_edges
            .into_iter()
            .filter(|(_, src, _)| live_regions.contains(src))
            .collect();

        RegionInfo {
            alloc_region: self.alloc_region,
            scope_region: self.scope_region,
            binding_region: self.binding_region,
            binding_source_regions: HashMap::new(),
            captured_reassigned_bindings: rustc_hash::FxHashSet::default(),
            sole_frame_held_regions: rustc_hash::FxHashSet::default(),
            live_regions,
            cross_region_refs,
            region_data: HashMap::new(),
            // Populated by the binding-chain extension in `analyze_regions_with`
            // (the tight, binding-resolved last-use per region). Empty here.
            binding_last_use: HashMap::new(),
            call_result_regions: self.call_result_regions,
            counted_cell_read_sites: self.counted_cell_read_sites,
            fresh_result_regions: self.fresh_result_regions,
            fiber_result_regions: self.fiber_result_regions,
            containment_edges,
            funnel_store_sites: self.funnel_store_sites,
            funnel_bytecopy_value_sites: self.funnel_bytecopy_value_sites,
            funnel_container_sites: self.funnel_container_sites,
            funnel_passthrough_sites: self.funnel_passthrough_sites,
            uncounted_read_sites: self.uncounted_read_sites,
            counted_read_aliases: self.counted_read_aliases,
            moves_out_release_sites: self.moves_out_release_sites,
            // Populated by the branch-compensation pass in `analyze_regions_with`
            // (the `-mut` sites where locus A released a container). Empty here.
            container_release_sites: rustc_hash::FxHashSet::default(),
            cell_release_regions: self.cell_release_regions,
            hard_edge_sites: self.hard_edge_sites,
            suppressed_decref_regions: rustc_hash::FxHashSet::default(),
            drop_on_overwrite_sites: rustc_hash::FxHashSet::default(),
            donated_overwrite_sites: rustc_hash::FxHashSet::default(),
            mutated_binding_value_regions: rustc_hash::FxHashSet::default(),
            reassigned_local_bindings: rustc_hash::FxHashSet::default(),
            cell_containers: HashMap::new(),
            begin_cell_regions: self.begin_cell_regions,
            // Populated by the `regions::merge` post-pass in `analyze_regions_with`,
            // after `region_data` decref_points are final (the seed's
            // coincident-decref_point gate reads them). Empty here.
            merged_parent: HashMap::new(),
            closure_cycle_members: FxHashSet::default(),
            // Populated by the `regions::merge` closure-cycle post-pass in
            // `analyze_regions_with` (the non-member body-tail release sites). Empty here.
            cycle_tail_release: HashMap::new(),
            // Populated by the `regions::ownership` post-pass in
            // `analyze_regions_with` (it reads the final `region_data` and
            // `merged_parent`). Empty here; empty after the pass too for a shape that
            // stays Shared (the RC baseline).
            owned_adopt_edges: HashMap::new(),
            capture_adopt_edges: HashMap::new(),
            cell_content_adopt_bindings: rustc_hash::FxHashSet::default(),
            // Populated by the `regions::ownership` post-pass in `analyze_regions_with`
            // (the co-owned-cycle and activation-owner cuts). Empty here; empty after
            // the pass too for a shape that stays Shared.
            owned_region_groups: HashMap::new(),
            owned_group_members: rustc_hash::FxHashSet::default(),
            transfer_adopt_regions: rustc_hash::FxHashSet::default(),
            activation_adopt_sites: HashMap::new(),
            // Populated by the `regions::compensate` post-pass in
            // `analyze_regions_with`, after `region_data` decref_points are final.
            // Empty here.
            branch_compensation: HashMap::new(),
            branch_arm_decrefs: HashMap::new(),
            stats,
        }
    }
}
