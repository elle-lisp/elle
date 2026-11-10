// audited: 2026-10-21
//! The order releases sharing one `decref_point` are emitted in: holder before holdee, so no release reads a freed page.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/adopt.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Set Tofte-Talpin region inference results.
    pub fn with_region_info(mut self, info: RegionInfo) -> Self {
        // Pre-index the two collections that `emit_increfs_for` /
        // `emit_decrefs_for` consult per HIR node, so each lookup is O(1)
        // instead of a linear scan (which made lowering O(n²) over a
        // large compilation unit like the stdlib).
        let mut increfs_by_site: HashMap<
            HirId,
            Vec<(crate::hir::region::Region, crate::hir::region::Region)>,
        > = HashMap::new();
        for &(site, src, dst) in &info.cross_region_refs {
            increfs_by_site.entry(site).or_default().push((src, dst));
        }
        let mut decrefs_by_decref_point: HashMap<HirId, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for (&r, d) in &info.region_data {
            decrefs_by_decref_point
                .entry(d.decref_point)
                .or_default()
                .push(r);
        }
        // The release order at each shared `decref_point`: the holder-before-holdee
        // edges `order_releases` sorts on. The adopt edges and the value-alias edges
        // are kept apart, because only the first set is a forest. Both are indexed by
        // their `earlier` endpoint once here, so the per-bucket sort costs the bucket's
        // own size rather than a scan of every edge in the unit.
        let mut adopt_owner: HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for &(member, owner) in info
            .owned_adopt_edges
            .values()
            .flatten()
            .chain(info.capture_adopt_edges.values().flatten())
        {
            adopt_owner.entry(member).or_default().push(owner);
        }
        let mut value_alias: HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for &(_site, alias, source) in info
            .counted_read_aliases
            .iter()
            .chain(info.opaque_result_aliases.iter())
            .chain(info.funnel_result_containers.iter())
        {
            value_alias.entry(alias).or_default().push(source);
        }
        for (&cell, &routed) in &info.cell_route_regions {
            value_alias.entry(routed).or_default().push(cell);
        }
        for regions in decrefs_by_decref_point.values_mut() {
            Self::order_releases(regions, &adopt_owner, &value_alias, &info);
        }
        // The 1-slot containers whose content drop lands at each scope node,
        // indexed the same way and for the same reason as the two above.
        let mut cell_drops_by_demise: HashMap<HirId, Vec<Binding>> = HashMap::new();
        for (&b, c) in &info.cell_containers {
            // A forwarding link's final content is the next link's to release,
            // and a module-scope cell's is the file-letrec teardown's; neither
            // emits a content drop (`CellContainer::drops_content`).
            if !c.drops_content() {
                continue;
            }
            cell_drops_by_demise.entry(c.demise).or_default().push(b);
        }
        // Deterministic emission order across runs (the map's iteration is not).
        for bindings in cell_drops_by_demise.values_mut() {
            bindings.sort_unstable_by_key(|b| b.0);
        }
        self.increfs_by_site = increfs_by_site;
        self.decrefs_by_decref_point = decrefs_by_decref_point;
        self.cell_drops_by_demise = cell_drops_by_demise;
        self.region_info = info;
        self
    }

    /// Order the releases sharing one `decref_point` (docs/impl/region/rules.md).
    ///
    /// A topological sort of the **holder-before-holdee** edges, so no release reads a
    /// page another release at the same point has already freed. Two edge sets:
    ///
    /// - **Adoption** (`adopt_owner`, `member → owner`). An adopted member keeps its OWN
    ///   `DecrefRegion`, a structural no-op only while the member is still `Owned`. Once
    ///   its owner's subtree drop reclaims it, that decref faults, so a member releases
    ///   before its owner. The edges come from `owned_adopt_edges` (store-adopted) and
    ///   `capture_adopt_edges` (capture-adopted). The two maps are disjoint per member,
    ///   so each member has one owner (src/hir/region/info.rs).
    /// - **Value aliasing** (`value_alias`, `alias → source`). A region that may be, or
    ///   live inside, another is released `DecrefValueRegion`-style. Its release reads
    ///   the value's own page, which the other's release can tear. The edges are the
    ///   relations the ownership cut's alias obligation closes over
    ///   (docs/impl/region/adopt.md). They are a native read's result and its container
    ///   (`counted_read_aliases`), and an opaque call's result and its arguments
    ///   (`opaque_result_aliases`). They also include a `Funnel`'s result and its
    ///   container (`funnel_result_containers`), and a cell-routed release's region and
    ///   the cell it loads (`cell_route_regions`). An opcode read mints no region and
    ///   contributes no edge, because the container's extended lifetime covers it.
    ///
    /// Nested subtrees and chained aliases resolve innermost-first by construction. A
    /// single flat priority class cannot express a transitive member ⊂ mid ⊂ root
    /// chain, nor a read whose container is a call's placeholder for the region that
    /// actually frees the page.
    ///
    /// Regions no edge relates are tie-broken by page-read depth. A value-gated
    /// `DecrefValueRegion` unwraps a cell to the inner value, so it reads deepest and
    /// sorts first (class 0). A `DecrefCellRegion` reads the cell header and frees the
    /// cell (class 1). A plain `DecrefRegion` frees and reads nothing (class 2). Region
    /// id breaks the final tie, so the order never depends on `HashMap` iteration
    /// (tests/impl/region-capture-cell-noreassign-uaf.lisp).
    ///
    /// **The two edge sets differ in what a cycle means.** Adoption is a forest, so a
    /// cycle there is an impossible state a debug assert flags. A read edge is only a
    /// MAY-alias. A binding whose `binding_regions` name several alternatives (a
    /// re-`def`ined or branch-union binding) makes each alternative the other's
    /// container. So two reads through it can point at each other, though no single
    /// runtime container does. Such a cycle is broken by re-sorting the stalled residue
    /// on the adopt edges alone, then by tie-break. That is deterministic, and never a
    /// release-build panic on a legal program.
    fn order_releases(
        regions: &mut Vec<crate::hir::region::Region>,
        adopt_owner: &HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>>,
        value_alias: &HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>>,
        info: &RegionInfo,
    ) {
        use crate::hir::region::Region;
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        // Page-read depth: lower sorts earlier. `cell_release_regions ⊆
        // call_result_regions`, so test the cell membership first.
        let class = |r: Region| -> u8 {
            if info.cell_release_regions.contains(&r) {
                1 // DecrefCellRegion: reads the cell page header, frees the cell
            } else if info.call_result_regions.contains(&r) {
                0 // DecrefValueRegion: unwraps the cell to the inner value (deepest read)
            } else {
                2 // plain DecrefRegion: frees, reads nothing
            }
        };
        // Kahn's algorithm over the edges restricted to `bucket`. A `later` region waits
        // on every in-bucket `earlier` region that holds it, and `succ` lists the waiters
        // of each earlier region. A member has one owner, but a region can be an adopted
        // member AND a read alias, so the successor set is a list. The edge sets arrive
        // indexed by their `earlier` endpoint, so a bucket costs its own size. Returns the
        // ordered prefix and the residue the edges never unblocked (a cycle).
        let kahn = |bucket: &[Region],
                    edges: &[&HashMap<Region, Vec<Region>>]|
         -> (Vec<Region>, Vec<Region>) {
            let present: rustc_hash::FxHashSet<Region> = bucket.iter().copied().collect();
            let mut indeg: HashMap<Region, u32> = bucket.iter().map(|&r| (r, 0)).collect();
            let mut succ: HashMap<Region, Vec<Region>> = HashMap::new();
            for &earlier in bucket {
                for &later in edges
                    .iter()
                    .filter_map(|e| e.get(&earlier))
                    .flatten()
                    .filter(|l| present.contains(l))
                {
                    *indeg.get_mut(&later).expect("later is in this bucket") += 1;
                    succ.entry(earlier).or_default().push(later);
                }
            }
            // Min-heap by (class, region id): the deterministic tie-break among the
            // currently-unblocked regions. `Region` has no `Ord`, so key on the id and
            // reconstruct — the id is unique within a bucket (`region_data` keys regions).
            let mut ready: BinaryHeap<Reverse<(u8, u32)>> = BinaryHeap::new();
            for &r in bucket {
                if indeg[&r] == 0 {
                    ready.push(Reverse((class(r), r.0)));
                }
            }
            let mut out: Vec<Region> = Vec::with_capacity(bucket.len());
            while let Some(Reverse((_, id))) = ready.pop() {
                let r = Region(id);
                out.push(r);
                for &waiter in succ.get(&r).into_iter().flatten() {
                    let d = indeg.get_mut(&waiter).expect("waiter tracked in indeg");
                    *d -= 1;
                    if *d == 0 {
                        ready.push(Reverse((class(waiter), waiter.0)));
                    }
                }
            }
            let residue: Vec<Region> = bucket
                .iter()
                .copied()
                .filter(|r| !out.contains(r))
                .collect();
            (out, residue)
        };

        let (mut out, residue) = kahn(regions, &[adopt_owner, value_alias]);
        if !residue.is_empty() {
            // A may-alias read cycle: drop the read edges over the stalled residue and
            // order it on the adopt forest alone.
            let (rest, cyclic) = kahn(&residue, &[adopt_owner]);
            out.extend(rest);
            debug_assert!(
                cyclic.is_empty(),
                "release-order ADOPT edges cycled at a shared decref_point: {cyclic:?}"
            );
            let mut cyclic = cyclic;
            cyclic.sort_by_key(|r| (class(*r), r.0));
            out.extend(cyclic);
        }
        *regions = out;
    }
}

/// Debug-only: no block frees an env cell before an instruction that reads through
/// that same cell.
///
/// A captured binding's value and its box are addressed by one env index. The
/// value's release loads the box RAW and unwraps it to the content, so it READS
/// the page the box's `DecrefCellRegion` frees. Two mechanisms hold that order,
/// and neither can see the other. The `decref_point` clamp places the box release
/// at or after every release routed through the cell (docs/impl/region/bindings.md).
/// The frame-exit relocation declines a move that would carry the box release back
/// across such a read (docs/impl/region/relocate.md). This states the property both
/// exist to hold, over the finished emission where the two meet.
#[cfg(debug_assertions)]
pub(super) fn assert_cells_outlive_their_readers(module: &FrozenModule) {
    for f in std::iter::once(&module.entry).chain(module.closures.iter()) {
        for b in f.view().blocks() {
            let mut from_index: HashMap<Reg, u16> = HashMap::new();
            let mut freed: HashMap<u16, usize> = HashMap::new();
            for (idx, i) in b.instrs().enumerate() {
                match i {
                    InstrRef::LoadCapture { dst, index }
                    | InstrRef::LoadCaptureRaw { dst, index } => {
                        from_index.insert(dst, index);
                        if let Some(&at) = freed.get(&index) {
                            panic!(
                                "env cell {index} is read at instruction {idx} of block \
                                 {:?}, after the DecrefCellRegion at {at} freed the box \
                                 — the read lands on a reclaimed page",
                                b.label()
                            );
                        }
                    }
                    InstrRef::DecrefCellRegion { src } => {
                        if let Some(&index) = from_index.get(&src) {
                            freed.insert(index, idx);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
