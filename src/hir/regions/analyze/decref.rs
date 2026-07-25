//! `decref_point` population passes.
//!
//! For each region `r`, `decref_point` is the structurally-latest program point
//! at which any value resolved to `r` is last used. These passes seed it from
//! per-HirId last-use analysis, then extend it through binding chains, hoist
//! env-cell releases past loops, and pin returned/destructured values to their
//! consuming node. Extracted verbatim from `analyze_regions_with`; the inline
//! comments explain the WHY of each pass.

// `super` is `hir::regions::analyze`; `super::super` reaches the sibling
// `hir::regions` items the original block saw through `use super::*`.
use super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::liveness::LastUseInfo;

/// Populate and extend `region_data[*].decref_point` across the several passes
/// that ran inline after `build_info`.
#[allow(clippy::too_many_arguments)]
pub(super) fn populate_decref_points(
    info: &mut RegionInfo,
    hir: &Hir,
    du: &DefUseBuilder,
    order: &HashMap<HirId, u32>,
    last_use_info: &LastUseInfo,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    return_sites: &[(HirId, Vec<Region>)],
    destructure_sites: &[(HirId, Vec<Region>)],
    break_sites: &[(HirId, Vec<Region>)],
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let last_use = &last_use_info.per_node;
    for (alloc_id, &region) in &info.alloc_region {
        let lu = last_use.get(alloc_id).copied().unwrap_or(*alloc_id);
        info.region_data
            .entry(region)
            .and_modify(|d| {
                if ord(lu) > ord(d.decref_point) {
                    d.decref_point = lu;
                }
            })
            .or_insert(RegionData { decref_point: lu });
    }
    // Pre-allocated capture cells (one region per cell, keyed by the Begin's
    // HirId in `begin_cell_regions` — not in `alloc_region`, which holds one
    // region per HirId). Base each cell region's `decref_point` on the Begin's
    // last use, exactly as an `alloc_region` entry at the Begin would get;
    // the binding-chain extension below then lifts it over the binding's own
    // uses. The base matters for a captured-but-never-used binding: without
    // it the region has no `region_data` entry, so no `DecrefRegion` is ever
    // emitted and the cell's initial reference leaks (Rule 8).
    for (begin_id, cells) in &info.begin_cell_regions {
        let lu = last_use.get(begin_id).copied().unwrap_or(*begin_id);
        for &(_b, region) in cells {
            info.region_data
                .entry(region)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Extend decref_point through binding chains: when a binding b holds a
    // value whose region r is somewhere else (e.g., `(let [result (let
    // [f ...] (array ok val))])`, `result`'s value lives in `array`'s
    // region — bound through the inner `let`'s body), the alloc-id
    // lookup above doesn't see r through b's uses because compute_last_use
    // only extends last_use for the binding's init HirId, not the
    // nested allocation's HirId. Without this extension r is freed at
    // the inner expression's tail, before b is ever read.
    //
    // For each binding b, find the max last_use among b's uses, and
    // extend region_data[r].decref_point for every region r in the
    // inference's binding_regions[b].
    let binding_uses = &du.uses;
    for (b, regions) in inference_binding_regions {
        if regions.is_empty() {
            continue;
        }
        let mut max_use = binding_uses
            .get(b)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
            .max_by_key(|id| ord(*id));
        // A binding captured by a lambda built inside a loop (while bound
        // outside it) must outlive the loop: its capture-use's last_use sits
        // inside the body, but the region demise must be hoisted to the loop
        // node, else it fires per iteration and frees the binding mid-loop
        // (region-loop-capture-squelch.lisp / supervisor-style UAF).
        if let Some(&ext) = last_use_info.capture_loop_ext.get(b) {
            if max_use.is_none_or(|cur| ord(ext) > ord(cur)) {
                max_use = Some(ext);
            }
        }
        if let Some(lu) = max_use {
            for &r in regions {
                info.region_data
                    .entry(r)
                    .and_modify(|d| {
                        if ord(lu) > ord(d.decref_point) {
                            d.decref_point = lu;
                        }
                    })
                    .or_insert(RegionData { decref_point: lu });
                // Record the binding-resolved (tight) last-use per region, for the
                // ownership lifetime obligation. Unlike
                // `region_data` above this is NOT max'd with the structural
                // alloc-site last-use, so a captured value's lambda-as-`let`-init
                // over-estimate — which the grow-only last-use fixpoint leaves
                // locked one step past the closure's last call, and the alloc-loop
                // then writes into `region_data` — does not leak in. `lu` already
                // reads the FINAL resolved `last_use` (the closure binding's last
                // use, not the lambda's structural position), so it is the tight
                // value the obligation needs. Max-by-order across a region's holder
                // bindings: a shared region must outlive every holder.
                info.binding_last_use
                    .entry(r)
                    .and_modify(|cur| {
                        if ord(lu) > ord(*cur) {
                            *cur = lu;
                        }
                    })
                    .or_insert(lu);
            }
        }
    }

    // ── Env-cell release: hoist past enclosing loops (once per activation) ──
    // An env cell — a captured local's `populate_env` cell or a captured param's
    // cell, marked in `cell_release_regions` — is minted EXACTLY ONCE per
    // activation (populate_env runs once when the activation is created), so its
    // `DecrefCellRegion` must fire exactly once per activation. The binding-chain
    // extension above can leave a cell-release region's `decref_point` at an
    // in-loop capture-use (the only use of a `@`-mutable local defined and
    // captured inside the loop), where it fires every iteration. For a closure
    // called in place and dying within the iteration, each iteration nets the box
    // region -1 (capture-incref +1, closure free-cascade -1, DecrefCellRegion -1)
    // — so the once-allocated box is freed at the end of iteration 1 and the next
    // iteration reads the recycled cell (the env-cell-in-loop UAF;
    // tests/elle/region-capture-cell-loop-uaf.lisp, cap2.lisp). Hoist each
    // cell-release region's `decref_point` to the OUTERMOST enclosing While/Loop,
    // which the lowerer emits AFTER the loop (the proven post-loop emission point
    // the bound-outside `capture_loop_ext` extension already targets) — once per
    // activation, matching the once-per-activation populate_env allocation.
    //
    // Sound for every env cell: the box is never re-allocated per iteration, so a
    // once-per-activation release can only over-keep (until the loop exits), never
    // mis-free. It composes with the closure-capture incref — an escaping
    // closure's reference keeps the box alive past the post-loop release, so the
    // box dies with the last surviving closure. This is the env-cell exception to
    // the value-binding rule the `capture_loop_ext` "bound outside" guard
    // enforces: a value bound INSIDE a loop is re-allocated per iteration and its
    // release must stay per-iteration, but an env cell's allocation is
    // loop-independent. See docs/impl/region/bindings.md "Env cells in loops:
    // release once per activation, not per iteration".
    if !info.cell_release_regions.is_empty() {
        let low = compute_subtree_low(hir, order);
        let mut iter_scopes: Vec<(HirId, u32, u32)> = Vec::new();
        collect_iter_scopes(hir, order, &low, &mut iter_scopes);
        if !iter_scopes.is_empty() {
            // Snapshot the cell regions first — the loop mutates `region_data`.
            let cell_regions: Vec<Region> = info.cell_release_regions.iter().copied().collect();
            for r in cell_regions {
                let Some(dp) = info.region_data.get(&r).map(|d| d.decref_point) else {
                    continue;
                };
                let dord = ord(dp);
                // Outermost enclosing loop = the iter-scope whose post-order
                // subtree interval `[low, order]` contains `dp` with the largest
                // `order` (an ancestor has the largest post-order index). `None`
                // if `dp` is in no loop — no hoist needed.
                let outermost = iter_scopes
                    .iter()
                    .filter(|&&(_, lo, hi)| lo <= dord && dord <= hi)
                    .max_by_key(|&&(_, _, hi)| hi)
                    .map(|&(id, _, _)| id);
                if let Some(loop_id) = outermost {
                    if ord(loop_id) > dord {
                        info.region_data.get_mut(&r).unwrap().decref_point = loop_id;
                    }
                }
            }
        }
    }

    // Extend each returned value's region `decref_point` to its `Return`
    // node. The lowerer emits the node's `IncrefValueRegion` before the
    // node's own `emit_decrefs_for`; pinning `decref_point` here guarantees a
    // freshly-allocated result region's `DecrefRegion` fires *after* the
    // retain (so the result survives its callee-side release and is
    // handed back with one owning reference). For a pass-through arg the
    // region is the callee's phantom scope (guard-suppressed) and this
    // is inert.
    for (return_id, regions) in return_sites {
        let lu = *return_id;
        for &r in regions {
            info.region_data
                .entry(r)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Extend each destructured value's regions' `decref_point` to its
    // `Destructure` node. A Destructure CONSUMES its value: the field
    // extraction (`StructGetOrNil` and friends) reads the value AFTER the
    // value expression's own last read, so a release anchored at the inner
    // read frees the source under the extraction. Bites exactly when no
    // destructured binding is used afterwards — the `&named`-param
    // prologue with unused params (docs/impl/region/rules.md Rule 4;
    // tests/elle/region-named-param-uaf.lisp, the lib/http2 import segv).
    for (destructure_id, regions) in destructure_sites {
        let lu = *destructure_id;
        for &r in regions {
            info.region_data
                .entry(r)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Extend each BROKEN value's regions' `decref_point` to where the `Block`
    // it was handed to is CONSUMED. A `Break` is the dual of the consuming nodes
    // above: it does not use its operand, it TRANSFERS it — the value becomes
    // the block's value, and dies wherever the block's value dies. Two things
    // make the pin necessary rather than a nicety:
    //
    //  - `break` lowers to a jump to the block's exit label, so a release
    //    anchored anywhere inside the body is emitted into the break's
    //    unreachable fall-through and never runs at all — the value is held to
    //    fiber teardown (the `break-value*` probes' former rate).
    //  - the block's own exit label is not late enough on its own: the block's
    //    value may flow straight into a consumer (`(f (block … (break v)))`),
    //    and releasing at the exit would free it under that consumer.
    //
    // `last_use[block]` is exactly "the node that consumes the block's value"
    // (its own id when nothing does), and the lowerer emits a node's decrefs
    // after it — after the exit label for the block itself. A binding that names
    // the block's value extends further through the ordinary binding chain,
    // and every extension is a max, so the latest wins
    // (docs/impl/region/mechanism.md § "`break` transfers its value; it does not
    // consume it"; tests/elle/region-break-transfer.lisp).
    for (block_id, regions) in break_sites {
        let lu = last_use.get(block_id).copied().unwrap_or(*block_id);
        for &r in regions {
            info.region_data
                .entry(r)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }
}

/// Collect every iterative-scope node (`While` or `Loop` — `while` lowers to
/// either) with its post-order subtree interval `[low, order]`, so containment
/// of a HirId is an interval test (`low <= ord(x) <= order`; see
/// `compute_subtree_low`). Used by the env-cell release hoist to find the
/// outermost loop enclosing a cell-release region's `decref_point`.
fn collect_iter_scopes(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<(HirId, u32, u32)>,
) {
    if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
        let lo = low.get(&hir.id).copied().unwrap_or(0);
        let hi = order.get(&hir.id).copied().unwrap_or(0);
        out.push((hir.id, lo, hi));
    }
    hir.for_each_child(|c| collect_iter_scopes(c, order, low, out));
}
