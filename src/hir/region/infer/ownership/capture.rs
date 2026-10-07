// audited: 2026-10-06
//! The capture edges the ownership forest reads, and the closure regions the co-owned group cut refuses.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/letrec.md
//! docs/impl/region/owner.md

use super::super::*;
use rustc_hash::FxHashSet;

/// Capture containment edges `(lambda_id, captured_region, closure_region)`: a
/// closure's region contains what it captures (`closure ⊇ captured`, the
/// orientation of `cross_region_refs`). `lambda_id` is the closure's HirId, the
/// construction site the lowerer keys a capture adopt at
/// (`RegionInfo::capture_adopt_edges`).
///
/// Capture records no `cross_region_refs` edge (see the `Lambda` arm of
/// `region::infer::walk`), so these edges are re-derived from the HIR. Only a
/// **live** captured region yields an edge, as `build_info` filters
/// `cross_region_refs`. A captured parameter resolves to a phantom region that owns
/// no allocation, which is a borrow the closure cannot own.
///
/// The edge follows how the captured binding is materialized:
///
/// - **By-value capture**: `closure ⊇ content`, at the binding's
///   `binding_source_regions`.
/// - **Prebound immutable letrec cell** (a compiled `MakeCaptureCell`): the closure
///   holds the cell, so the edge is `closure ⊇ cell`, at `single_cell_region_of`.
/// - **Re-storable cell** (`is_restorable_capture_cell`): no edge; the capture stays
///   a borrow.
/// - **`populate_env` env cell** (no compiled cell): no edge; the runtime env owns
///   the cell.
pub(in crate::hir::region::infer) fn capture_containment_edges(
    hir: &Hir,
    info: &RegionInfo,
    arena: &BindingArena,
) -> Vec<(HirId, Region, Region)> {
    fn walk(
        h: &Hir,
        info: &RegionInfo,
        arena: &BindingArena,
        out: &mut Vec<(HirId, Region, Region)>,
    ) {
        if let HirKind::Lambda(lambda) = &h.kind {
            if let Some(&closure_r) = info.alloc_region.get(&h.id) {
                for c in &lambda.captures {
                    let bi = arena.get(c.binding);
                    // A re-storable cell's content lives only until the next rebind,
                    // shorter than the cell, so the capture takes no owner edge.
                    if bi.is_restorable_capture_cell() {
                        continue;
                    }
                    if bi.needs_capture() {
                        // A prebound immutable letrec cell: point the edge at the cell.
                        // `single_cell_region_of` yields `None` for a `populate_env`
                        // route or a binding with several cells, which stays a borrow.
                        // The lowerer applies the same test, so its `AdoptCellRegion`
                        // names the same cell.
                        if let Some(cell_r) = info.single_cell_region_of(c.binding) {
                            if cell_r != closure_r && info.live_regions.contains(&cell_r) {
                                out.push((h.id, cell_r, closure_r));
                            }
                        }
                        continue;
                    }
                    // A by-value capture: `closure ⊇ content`, pointed at the content.
                    let Some(regions) = info.binding_source_regions.get(&c.binding) else {
                        continue;
                    };
                    for &r in regions {
                        if r != closure_r && info.live_regions.contains(&r) {
                            out.push((h.id, r, closure_r));
                        }
                    }
                }
            }
        }
        h.for_each_child(|c| walk(c, info, arena, out));
    }
    let mut out = Vec::new();
    walk(hir, info, arena, &mut out);
    out
}

/// Regions that hold a **closure** (a `Lambda`'s `alloc_region`), which
/// `compute_owned_region_groups` refuses. The co-owned group cut owns store-only
/// cycles, and a cycle with a closure in it belongs to another mechanism:
///
/// - A **`letrec` closure clique** (a forward cell holds a closure, and sibling
///   closures capture the cell) belongs to the closure-cycle merge
///   (`region::infer::merge`), which runs first. A clique the merge refuses is refused
///   here too. The group free resolves each member with `result_region_of`, which
///   unwraps a `CaptureCell` to its content, so it cannot name a cell's own region.
/// - A **capture-back-edge SCC** (a container captured by a closure it holds) belongs
///   to the activation cut (`compute_activation_adopts`).
///
/// A non-cyclic local capture clique `closure ⊇ cell ⊇ content` never reaches the
/// group cut; the rooted-subtree cut (`compute_owned_subtrees`) owns it.
pub(super) fn closure_regions(hir: &Hir, info: &RegionInfo) -> FxHashSet<Region> {
    fn walk(h: &Hir, info: &RegionInfo, out: &mut FxHashSet<Region>) {
        if matches!(h.kind, HirKind::Lambda(_)) {
            if let Some(&r) = info.alloc_region.get(&h.id) {
                out.insert(r);
            }
        }
        h.for_each_child(|c| walk(c, info, out));
    }
    let mut out = FxHashSet::default();
    walk(hir, info, &mut out);
    out
}
