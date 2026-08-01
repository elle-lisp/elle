use super::super::*;
use rustc_hash::FxHashSet;

/// Capture containment edges `(lambda_id, captured_value_region, closure_region)` — a
/// closure's region contains every value it captures (`closure ⊇ captured`, the same
/// `source → target` orientation as `cross_region_refs`). The `lambda_id` is the
/// closure's HirId, the **construction site** an `AdoptRegion` for a capture edge hangs
/// on (capture has no `cross_region_refs` store site, so the lowerer keys its adopt by
/// the Lambda; `RegionInfo::capture_adopt_edges`).
///
/// These are re-derived from the HIR because capture records **no** `cross_region_refs`
/// edge: the RC double-count fix relies on the runtime auto-incref over the `Closure`
/// env instead of a static `IncrefRegion` (`capture_records_no_cross_region_edge`; the
/// Lambda arm of `region::infer::walk`). The closure's region is its `alloc_region` (every
/// Lambda gets one — `alloc_here(hir.id)` in the walk); each captured value's regions
/// are the captured binding's `binding_source_regions`.
///
/// Only **live** source regions yield an edge, mirroring `build_info`'s filter on
/// `cross_region_refs`: a captured *parameter* or capture-cell resolves to a phantom
/// region (not in `live_regions`, in `call_result_regions`) that owns no allocation —
/// the caller/env owns it, so it is a borrow, not a containment the closure can own.
///
/// A capture is resolved by its binding's materialization:
///
/// - **By-value capture** (an immutable, non-prebound local): the closure holds the value
///   directly — a genuine containment `closure ⊇ content`, so the edge points at the
///   captured value's `binding_source_regions` (the content region), exactly as a store
///   edge does. This is the forest's per-call closure↔capture reclamation.
/// - **Prebound immutable letrec cell** (`needs_capture` but not re-storable — a compiled
///   `MakeCaptureCell` forward reference): the closure holds the CELL, not the content. The
///   edge is re-pointed at the **cell region** (`begin_cell_regions`), yielding
///   `closure ⊇ cell`. Paired with the walk's `cell ⊇ content` edge, external uniqueness
///   sees the true chain `closure ⊇ cell ⊇ content`, so a local, non-escaping clique
///   `{closure, cell, content}` reclaims as a unit (the cell's own arena included) by the
///   closure's subtree drop — the interior cell↔closure cycle reclaimed with it. An
///   escaping/externally-referenced clique fails external uniqueness and stays Shared.
///   Because the cell store is uncounted (no `cross_region_refs` edge), this edge is the
///   ONLY way the scan sees the cell is held; the runtime realizes the adopt via
///   `AdoptCellRegion` (`region_of` the cell, not the unwrapped content).
/// - **Re-storable cell** (`is_restorable_capture_cell` — an `@`-mutable captured local or
///   a mutated captured parameter): yields **no** edge. Its content lifetime is per-rebind
///   (shorter than the cell's, whose release is hoisted past enclosing loops), so adopting
///   the content into the cell's subtree would free a displaced prior under a live cell —
///   the loop over-free (`region-capture-cell-loop-uaf.lisp`). The re-storable cell stays a
///   borrow (the content reclaims on the per-region-RC baseline). `compute_adopt_edges`
///   independently refuses to adopt a re-storable cell's `cell ⊇ content` edge (§3 gate).
/// - **`populate_env` env cell** (an in-lambda captured binding with no compiled cell): a
///   phantom region the runtime env owns (no `begin_cell_regions` entry), so it yields no
///   edge and stays a borrow.
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
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if let Some(&closure_r) = info.alloc_region.get(&h.id) {
                for c in captures {
                    let bi = arena.get(c.binding);
                    // A re-storable cell capture is a borrow through a separately-owned
                    // env cell whose content lifetime is per-rebind (shorter than the
                    // cell's) — no owner edge (see the doc above; §3 hazard).
                    if bi.is_restorable_capture_cell() {
                        continue;
                    }
                    if bi.needs_capture() {
                        // A prebound immutable letrec cell: re-point the edge at the CELL
                        // region so external uniqueness sees `closure ⊇ cell` (paired with
                        // the walk's `cell ⊇ content`). `single_cell_region_of` yields the
                        // cell only when the binding minted exactly one — a `populate_env`
                        // route (no compiled cell) OR an ambiguous multi-cell double-declare
                        // yields `None` and stays a borrow. The lowerer applies the SAME
                        // gate, so the emitted `AdoptCellRegion` names the same cell.
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

/// Regions that hold a **closure** (a `Lambda`'s `alloc_region`) — the members
/// `compute_owned_region_groups` refuses, holding the co-owned-group free to its charter:
/// **store-only** runtime cycles (a bare `@array ↔ @array` knot with no closure member). A
/// closure-involving cycle is owned by a *different* mechanism, so routing it here would be
/// unsound or redundant. This refusal is a **mechanism boundary**, not a conservatism.
///
/// - A **`letrec` closure clique** — a forward cell holds the closure (`StoreCaptureCell`)
///   and the sibling closures capture the cell (the `cell ↔ closure` cycle) — is collapsed
///   onto one arena by the closure-cycle **MERGE** (`region::infer::merge`, O(1), which runs before
///   this pass), so its members resolve through `merged_root` and never reach the group walk
///   as independent regions. A merge-REFUSED such clique is refused *here* too, because the
///   group free resolves each member value with `result_region_of`, which UNWRAPS a
///   `CaptureCell` to its content — so it cannot name a cell member's OWN region, and freeing
///   the set would dangle the cell and over-free (a `--trace=guardfree` UAF). The group cut is
///   structurally the wrong owner for a cell-bearing cycle; the merge is the right one (and the
///   `targets.len() != scc.len()` gate below independently refuses a multi-cell letrec clique,
///   whose cells share one `begin_cell_regions` mint site).
/// - The **capture-back-edge SCC** — a container captured by a closure it holds (`m ⊇ c`
///   store, `c ⊇ m` capture) — is claimed by the ACTIVATION cut
///   (`compute_activation_adopts`), whose owner-node completion release frees it. Refusing
///   closure regions here is exactly what leaves it for that cut (pinned by
///   `activation_adopts_capture_back_edge_scc` and `activation_adopt_excludes_other_mechanisms`:
///   a store-only cycle goes to the group free, a capture cycle to the activation node).
///
/// The `closure ⊇ cell ⊇ content` containment the walk now records
/// (`capture_containment_edges` re-pointed through the cell + the `cell ⊇ content` edge) does
/// NOT move a cyclic closure clique into the group cut; it lets the rooted-subtree cut
/// (`compute_owned_subtrees`) reclaim a **non-cyclic** local capture clique via
/// `AdoptCellRegion` (pinned by `owned_subtrees_admits_local_capture_cell_clique`).
pub(super) fn closure_regions(hir: &Hir, info: &RegionInfo) -> FxHashSet<Region> {
    fn walk(h: &Hir, info: &RegionInfo, out: &mut FxHashSet<Region>) {
        if matches!(h.kind, HirKind::Lambda { .. }) {
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
