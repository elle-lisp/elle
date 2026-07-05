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
/// Lambda arm of `regions::walk`). The closure's region is its `alloc_region` (every
/// Lambda gets one — `alloc_here(hir.id)` in the walk); each captured value's regions
/// are the captured binding's `binding_source_regions`.
///
/// Only **live** source regions yield an edge, mirroring `build_info`'s filter on
/// `cross_region_refs`: a captured *parameter* or capture-cell resolves to a phantom
/// region (not in `live_regions`, in `call_result_regions`) that owns no allocation —
/// the caller/env owns it, so it is a borrow, not a containment the closure can own.
///
/// A capture of a **re-storable cell** (`is_restorable_capture_cell()` — a `@`-mutable
/// captured local or a mutated captured parameter, materialized as a `CaptureCell`/
/// `populate_env` env cell) yields **no** edge: the closure captures the CELL by
/// indirection, a BORROW through a separately-owned env cell, not a containment of the
/// cell's contents. `binding_source_regions` records where the value *points* (the live
/// content region), not the phantom cell region, so the `live_regions` filter alone would
/// let the content through and fold it into the closure's Owned subtree. That is unsound:
/// the env cell is minted once per activation and its release is hoisted past enclosing
/// loops, so it (and its contents, held by an uncounted cell store the ownership scan
/// cannot see) outlives any per-iteration closure — the closure's subtree drop would free
/// the cell's contents while the persistent cell still references them, and the next
/// iteration's re-store derefs the freed page (`region-capture-cell-loop-uaf.lisp`).
/// The cell owns its contents and releases them; the closure only
/// reads through it. A prebound *immutable* letrec cell is excluded (its content is set
/// once, not re-stored): that closure-cycle structure is governed by the closure-cycle
/// merge and the lifetime obligation instead.
pub(super) fn capture_containment_edges(
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
                    // A re-storable cell capture is a borrow through a separately-owned
                    // env cell, not a containment the closure owns (see the doc above).
                    if arena.get(c.binding).is_restorable_capture_cell() {
                        continue;
                    }
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

/// Regions that hold a **closure** (a `Lambda`'s `alloc_region`). A `letrec` self/mutual
/// recursive closure is not a clean region cycle: it is a capture-cell↔closure structure —
/// the forward-reference cell holds the closure (`StoreCaptureCell`) and the closure
/// captures the cell. Crucially the **cell⊇closure containment records no `cross_region_ref`
/// edge** (capture-cell stores are uncounted, like ordinary capture), so it is *invisible*
/// to the external-uniqueness scan: a co-owned SCC over the closure regions looks externally
/// unique while the cell still references the closures from outside the SCC. Freeing the SCC
/// wholesale then dangles the cell, and the cell's own `DecrefRegion` over-frees — a
/// use-after-free that `--trace=guardfree` detonates under the full stdlib (the
/// `region_ownership_reclaims_*_recursion_closure_cycle` runtime tests pin the shapes; the
/// `without_stdlib` plain-VM harness reads the freed-but-intact page and masks it).
///
/// Until the cell⊇closure containment is modeled (so external uniqueness can see it),
/// [`compute_owned_region_groups`] **refuses** any cycle touching a closure region — it
/// stays Shared (per-region RC: leaks an immutable closure cycle, which is acceptable; a
/// UAF is not). This is the always-legal baseline, correct by construction. The
/// capture-back-edge subset (a container captured by a closure it holds — an SCC whose
/// interior edges mix capture AND store) does not rest there: `compute_activation_adopts`
/// claims it for the activation owner node, whose completion release frees the cycle
/// wholesale (no cell is involved — the closure is `let`-bound, not a letrec forward
/// reference — so the invisible-containment hazard above does not arise).
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
