// audited: 2026-09-05
//! The order releases sharing one `decref_point` are emitted in: holder before
//! holdee, so no release reads a page another already freed.
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
        // The **release order** at each shared `decref_point` (docs/impl/region/rules.md
        // Rule 4): the `(earlier, later)` edges of every region that HOLDS another's pages
        // live, so the holder's release is emitted first. Two sources:
        //
        // - **Adoption.** An adopted member keeps its OWN `DecrefRegion`, a structural
        //   no-op only while the member is still `Owned` — once its owner's subtree drop
        //   reclaims it, that decref faults. So a member releases before its owner. The
        //   adopt maps hold exactly those edges: `owned_adopt_edges` (store-adopted, each
        //   store site → `(member, owner)`) and `capture_adopt_edges` (capture-adopted,
        //   each closure site → `(captured, closure)`), disjoint per member — a member is
        //   adopted by its single owner through exactly one map (region/info.rs).
        // - **Value aliasing.** A region that may be — or live inside — another is
        //   released `DecrefValueRegion`-style, resolving its runtime region by READING
        //   the value's own page, which the other's release can tear. Where the two land
        //   on one point (a discarded read, whose alias dies exactly where its container
        //   does), the alias must be ordered ahead. The same three relations the ownership
        //   cut's alias obligation closes over supply these edges, oriented
        //   `alias → source` throughout (docs/impl/region/adopt.md): a native
        //   read's result and the container it read from
        //   (`counted_read_aliases`), an opaque call's result and its arguments
        //   (`opaque_result_aliases`), and a `Funnel`'s result and its container
        //   (`funnel_result_containers`). They compose transitively through the sort, so a
        //   read out of a CALL's result — whose recorded container is the call's
        //   placeholder, not the container the call handed back — is still ordered ahead
        //   of the container that frees the page. An opcode read mints no region of its
        //   own and contributes no edge; its borrow is covered by the container's extended
        //   lifetime instead (region/rules.md Rule 4).
        //
        // `order_releases` topologically sorts the result (holder before holdee, nested
        // subtrees innermost-first); a single flat priority class cannot express a
        // transitive member ⊂ mid ⊂ root chain, nor a region that both is adopted and
        // reads out of another. The two sets stay SEPARATE because only the first is a
        // forest: a binding whose `binding_regions` name several alternatives (a
        // re-`def`ined or branch-union binding) makes two reads each other's container, so
        // the read edges can carry a may-alias cycle no order satisfies — which the sort
        // resolves by tie-break rather than treating as the impossible state an adopt-edge
        // cycle would be.
        // Both sets are indexed by their `earlier` endpoint once here, so the per-bucket
        // sort below costs the bucket's own size rather than a scan of every edge in the
        // unit (the O(n²)-over-the-stdlib trap the two indices above exist to avoid).
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
        for regions in decrefs_by_decref_point.values_mut() {
            Self::order_releases(regions, &adopt_owner, &value_alias, &info);
        }
        // The fn-local 1-slot containers whose content drop lands at each scope
        // node, indexed the same way and for the same reason as the two above.
        let mut cell_drops_by_demise: HashMap<HirId, Vec<Binding>> = HashMap::new();
        for (&b, c) in &info.cell_containers {
            // A cell that forwards its final content into the next link of a
            // loop chain has no content drop of its own: that link took the one
            // reference over and releases it (`CellContainer::forwards_content`).
            if c.forwards_content {
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

    /// Order the releases sharing one `decref_point` (docs/impl/region/rules.md Rule 4).
    ///
    /// A topological sort of the **holder-before-holdee** edges — `adopt_owner`
    /// (`member → owner`, the single-owner Owned-subtree forest) and `value_alias`
    /// (`alias → source`, every region that may be or live inside another: a borrowing
    /// read's result, an opaque call's result, a funnel's pass-through result) — so every
    /// store/capture-adopted member's own `DecrefRegion` — a no-op only while the member
    /// is still `Owned` — and every alias's page-reading `DecrefValueRegion` are emitted
    /// before the release that frees (or subtree-drops) what they name
    /// (docs/impl/region/adopt.md). Both nested subtrees and chained
    /// aliases resolve innermost-first by construction — a single flat priority class
    /// cannot express a transitive member-before-owner chain, nor a read whose container
    /// is a call's placeholder for the region that actually frees the page.
    ///
    /// Regions no such edge relates are tie-broken by page-read depth: a value-gated
    /// `DecrefValueRegion` that unwraps a cell to the inner value reads deepest and sorts
    /// first (class 0), then a `DecrefCellRegion` that reads the cell header and frees the
    /// cell (class 1), then a plain `DecrefRegion` that frees and reads nothing (class 2);
    /// region id breaks the final tie, so the order never depends on `HashMap` iteration
    /// (the flaky capture-cell UAF, region-capture-cell-noreassign-uaf.lisp).
    ///
    /// **The two edge sets differ in what a cycle means.** Adoption is a forest — each
    /// member has exactly one owner (the two adopt maps are disjoint per member;
    /// region/info.rs) — so a cycle there is an impossible state a debug assert flags. A
    /// read edge is only a MAY-alias: a binding whose `binding_regions` name several
    /// alternatives (a re-`def`ined or branch-union binding) makes each alternative the
    /// other's container, so two reads through it can point at each other though no
    /// single runtime container does. Such a cycle is broken by re-sorting the stalled
    /// residue on the adopt edges alone, then by tie-break — deterministic, never a
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
        // on every in-bucket `earlier` region that holds it; `waiters` lists the waiters of
        // each earlier region (a member has one owner, but a region can be an adopted
        // member AND a read alias, so the successor set is a list). The edge sets arrive
        // PRE-INDEXED by their `earlier` endpoint, so a bucket costs its own size, not a
        // scan of every edge in the unit — the same O(n²)-over-the-stdlib trap the
        // decref/incref indices above exist to avoid. Returns the ordered prefix and the
        // residue the edges never unblocked (a cycle).
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
/// A captured binding's value and its box are addressed by one env index, and the
/// value's release loads the box RAW and unwraps it to the content — so it READS
/// the page the box's `DecrefCellRegion` frees. Two mechanisms hold that order and
/// neither can see the other: the `decref_point` clamp places the box release at or
/// after every release routed through the cell (docs/impl/region/bindings.md), and
/// the frame-exit relocation declines a move that would carry the box release back
/// across such a read (docs/impl/region/relocate.md). This states the property both exist to
/// hold, over the finished emission where the two meet.
#[cfg(debug_assertions)]
pub(super) fn assert_cells_outlive_their_readers(module: &LirModule) {
    for f in std::iter::once(&module.entry).chain(module.closures.iter()) {
        for b in &f.blocks {
            let mut from_index: HashMap<Reg, u16> = HashMap::new();
            let mut freed: HashMap<u16, usize> = HashMap::new();
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::LoadCapture { dst, index }
                    | LirInstr::LoadCaptureRaw { dst, index } => {
                        from_index.insert(*dst, *index);
                        if let Some(&at) = freed.get(index) {
                            panic!(
                                "env cell {index} is read at instruction {idx} of block \
                                 {:?}, after the DecrefCellRegion at {at} freed the box \
                                 — the read lands on a reclaimed page",
                                b.label
                            );
                        }
                    }
                    LirInstr::DecrefCellRegion { src } => {
                        if let Some(&index) = from_index.get(src) {
                            freed.insert(index, idx);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
