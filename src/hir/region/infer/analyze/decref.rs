// audited: 2026-09-21
//! `decref_point` population: the ordered passes that decide, for each region,
//! the program point its release is emitted at.
//!
//! docs/impl/region/anchors.md
//!
//! For each region `r`, `decref_point` is the structurally-latest program point
//! at which any value resolved to `r` is last used. These passes seed it from
//! per-HirId last-use analysis, then extend it through binding chains, hoist
//! env-cell releases past loops, and pin returned/destructured values to their
//! consuming node. The inline comments explain the WHY of each pass.

// `super` is `hir::region::infer::analyze`, so `super::super` is
// `hir::region::infer` and the glob brings in the solver's own types.
use super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::liveness::LastUseInfo;
use crate::hir::region::{PinDecref, ProgramOrder};

// Each window is its own subject — what it moves, and what bounds it. The
// driver below runs them in the one order their premises hold in: each reads
// the FINAL `decref_point` the passes before it left.
mod branch;
mod branchscopes;
mod breakwindow;
mod cells;

use branch::pin_branch_arm_releases;
use breakwindow::pin_break_skipped_releases;
use cells::{
    collect_iter_scopes, innermost_covering, pin_cell_release_after_routed_releases,
    post_loop_placement,
};

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
    break_skip_blocks: &[(HirId, Vec<HirId>)],
    frame_replacing_tail_calls: &rustc_hash::FxHashSet<HirId>,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let porder = ProgramOrder::new(order);
    let last_use = &last_use_info.per_node;
    for (alloc_id, &region) in &info.alloc_region {
        let lu = last_use.get(alloc_id).copied().unwrap_or(*alloc_id);
        info.region_data.pin_to(region, lu, porder);
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
        info.region_data
            .pin_all_to(cells.iter().map(|&(_b, region)| region), lu, porder);
    }

    // The collection a rest pattern BUILDS (`pattern_rest_regions`, keyed by
    // the `Destructure`/`Match` node rather than held in `alloc_region`, which
    // is one region per HirId). The base is the node itself: a rest name
    // nothing reads leaves the binding chain below with no use to extend a
    // release over, and a region with no `region_data` entry gets no release
    // emitted at all — the shape that provokes the defect most often
    // (docs/impl/region/anchors.md § "A rest pattern's collection is built, not
    // read out").
    //
    // The node, not its last use. A `Match` node's own last use is wherever the
    // match's VALUE goes, which is a different value from the collection an arm
    // built — extending to it would carry the release past a loop that builds a
    // fresh collection per iteration. The node is post-ordered after every arm
    // body, which is all the base has to reach.
    for (node_id, rests) in &info.pattern_rest_regions {
        info.region_data
            .pin_all_to(rests.iter().map(|c| c.region), *node_id, porder);
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

    // Post-order subtree intervals `[low, order]`, so containment of a HirId is
    // an interval test, and every iterative scope's. Computed once for the four
    // cell passes that read them — the fn-local container's covering node and its
    // demise hoist, the env cell's once-per-activation hoist, and the clamp that
    // follows the value releases routed through an env cell — and skipped
    // entirely when the unit has no cell of either kind.
    let mut subtree_low: HashMap<HirId, u32> = HashMap::new();
    let mut iter_scopes: Vec<(HirId, u32, u32)> = Vec::new();
    if !info.cell_containers.is_empty() || !info.cell_release_regions.is_empty() {
        subtree_low = compute_subtree_low(hir, order);
        collect_iter_scopes(hir, order, &subtree_low, &mut iter_scopes);
    }

    // ── The fn-local 1-slot container's content drop ──────────────────────
    // The cell's own reference to its current content dies at its last access —
    // the latest of its reads and of the node COVERING its writes — with one
    // hoist (docs/impl/region/bindings.md § "Where the content drop lands").
    //
    // A single write is that covering node itself. Several are not: a cell
    // reached from mutually exclusive arms has its latest write inside ONE arm,
    // and a drop there runs on that path alone, leaving every other arm's value
    // held by the cell and released nowhere. The writes' innermost covering node
    // — their lowest common ancestor — is the nearest point after all of them
    // that every storing path reaches.
    //
    // The hoist: a cell CARRIED ACROSS a loop is re-pointed every iteration, so
    // a drop inside the body would free the content the next iteration reads.
    // Such a cell is a loop PARAMETER, i.e. its scope node is the loop itself, so
    // hoisting to that node lands the one drop after the loop — where the lowerer
    // emits the loop's own releases. A cell bound INSIDE a loop body has a body
    // scope node instead, so it is not hoisted and drops once per iteration,
    // matching its per-iteration mint. And a loop's parameters stay readable past
    // the loop (the `(while … (assign acc …)) acc` idiom), which is why the hoist
    // is a max and not a move.
    if !info.cell_containers.is_empty() {
        let loop_ids: rustc_hash::FxHashSet<HirId> =
            iter_scopes.iter().map(|&(id, _, _)| id).collect();
        // Each scope node by the region it introduces, so a binding's scope node
        // is one lookup through `binding_region`.
        let scope_of_region: HashMap<Region, HirId> =
            info.scope_region.iter().map(|(&id, &r)| (r, id)).collect();
        let carried_loop: HashMap<Binding, HirId> = info
            .cell_containers
            .keys()
            .filter_map(|&b| {
                let scope = *info.binding_region.get(&b)?;
                let node = *scope_of_region.get(&scope)?;
                loop_ids.contains(&node).then_some((b, node))
            })
            .collect();
        for (b, c) in info.cell_containers.iter_mut() {
            let store_ords: Vec<u32> = c.stores.sites().map(ord).collect();
            let covering = innermost_covering(hir, order, &subtree_low, &store_ords);
            let latest = du
                .uses
                .get(b)
                .into_iter()
                .flat_map(|v| v.iter())
                .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
                .chain(covering)
                .chain(carried_loop.get(b).copied())
                .max_by_key(|id| ord(*id));
            if let Some(lu) = latest {
                c.demise = lu;
            }
        }
    }

    // Snapshot both cell views before the passes below start mutating
    // `region_data`: the value regions to hold back from the binding chain, and
    // the store-site pins that replace them.
    //
    // The hold-back is keyed on "is this binding a cell", not on "is this cell's
    // own value region", because a cell binding names a SLOT rather than any one
    // value — so no cell's stored value may ride any cell binding's uses. A chain
    // of forwarding loop parameters is where the two readings diverge: the
    // downstream link's source regions include the upstream link's value regions
    // (the `Loop` init copies them), and its own last use sits past the loop that
    // stores them, so one release would cover N allocations
    // (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
    // reference along, so the fold follows it whole"). An ANF producer temp is
    // not a cell binding and still extends normally, which is what keeps the
    // release after the allocation it names.
    let cell_value_regions: rustc_hash::FxHashSet<Region> = info
        .cell_containers
        .values()
        .flat_map(|c| c.stores.value_regions())
        .collect();
    // Each stored value region beside the point its CELL stops holding it, which
    // the uncounted-read pass reads to tell a borrow the cell already covers from
    // one it does not. Every link of a forwarding chain records only the values it
    // stores itself, so this names the link that stored the value rather than the
    // one that finally drops it — the earlier of the two, and the safe direction
    // to be wrong in. The max is defensive: one region, one storing cell.
    let mut cell_drop_point: HashMap<Region, HirId> = HashMap::new();
    for c in info.cell_containers.values() {
        for r in c.stores.value_regions() {
            cell_drop_point
                .entry(r)
                .and_modify(|cur| {
                    if porder.is_after(c.demise, *cur) {
                        *cur = c.demise;
                    }
                })
                .or_insert(c.demise);
        }
    }
    // One pin per STORE, carrying that store's own value regions. A cell reached
    // from two mutually exclusive arms stores a different value at each site, so
    // pinning every value at the cell's last store puts the first arm's release on
    // a path that arm does not reach (docs/impl/region/bindings.md § "The store
    // site is the store that took THAT value"). Where one region really is stored
    // at several sites, every one of them pins it and the pin rule's maximum picks
    // the latest — the point after every store that took a reference of it.
    let cell_store_pins: Vec<(HirId, Vec<Region>)> = info
        .cell_containers
        .values()
        .flat_map(|c| c.stores.iter())
        .map(|s| (s.site, s.value_regions.clone()))
        .collect();
    for (b, regions) in inference_binding_regions {
        if regions.is_empty() {
            continue;
        }
        let names_a_cell = info.cell_containers.contains_key(b);
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
                // A fn-local 1-slot container's stored values do NOT ride a CELL
                // binding's uses: such a binding names the slot, not any one
                // value, so extending here would put one release at the cell's
                // last use — which, in a loop that re-mints the content every
                // iteration, can only reach whichever value the producer slot
                // happens to hold last. The cell's own counted reference covers
                // the value from the store onward, so the producer's claim is
                // dead AT the store and is pinned there below
                // (docs/impl/region/bindings.md § "Reassigned mutable bindings
                // are 1-slot containers").
                if !(names_a_cell && cell_value_regions.contains(&r)) {
                    info.region_data.pin_to(r, lu, porder);
                }
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
    // loop-independent. See docs/impl/region/cells.md "Env cells in loops:
    // release once per activation, not per iteration".
    if !info.cell_release_regions.is_empty() && !iter_scopes.is_empty() {
        // Snapshot the cell regions first — the loop mutates `region_data`.
        let cell_regions: Vec<Region> = info.cell_release_regions.iter().copied().collect();
        for r in cell_regions {
            let Some(dp) = info.region_data.get(&r).map(|d| d.decref_point) else {
                continue;
            };
            if let Some(loop_id) = post_loop_placement(dp, &iter_scopes, order) {
                info.region_data.get_mut(&r).unwrap().decref_point = loop_id;
            }
        }
    }

    // Extend each UNCOUNTED-read container's regions' `decref_point` to where the READ's
    // result is last used. An opcode element read (`%get`/`%first`/`%rest` —
    // `uncounted_read_sites`) hands back a value that still lives inside the container
    // and raises no count on it, so the container's lifetime is the borrow's only
    // protection: its last use is the READER's, not the read's. Anchored at the read, the
    // container's free-time cascade drops the element's last count and the reader derefs
    // a freed page. `last_use` at the read site is exactly "where the read's result is
    // last used" — the binding chain resolves it through a named result, the enclosing
    // consumer when ANF leaves the read unnamed in operand position, which is the case no
    // other pass covers (docs/impl/region/rules.md Rule 4, the borrowing node).
    //
    // A NATIVE read is absent here on purpose: its dispatch takes the Rule 5 pass-through
    // retain, so the reader holds its own counted reference and extending the container
    // would be a pure over-keep. What that retain cannot survive is adoption freezing the
    // member's RC — handled where that decision is made, in the ownership cut
    // (`counted_read_aliases`, region/adopt.md § "The lifetime obligation the root
    // carries"). A moves-out REMOVE is excluded from both: it extracts its element
    // instead of borrowing it.
    //
    // A value stored into a fn-local 1-slot container has a SECOND protector the
    // extension does not need to duplicate: the producer's claim is discharged at
    // the store, and from there the cell's own counted reference is what keeps
    // the value — and any borrow out of it — alive. Where the cell drops that
    // reference at or after the borrow dies, the extension buys nothing and costs
    // a great deal: it drags the producer's release past a loop that stores a
    // fresh value every iteration, so one release covers N allocations
    // (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
    // reference along, so the fold follows it whole"). Where the cell drops it
    // EARLIER — the borrow flows on past the cell's own last access — the
    // producer's reference is the borrow's only protection and keeps the
    // extension.
    for (read_id, container_regions) in &info.uncounted_read_sites {
        let lu = last_use.get(read_id).copied().unwrap_or(*read_id);
        info.region_data.pin_all_to(
            container_regions.iter().copied().filter(|r| {
                cell_drop_point
                    .get(r)
                    .is_none_or(|&drop| porder.is_after(lu, drop))
            }),
            lu,
            porder,
        );
    }

    // Pin each value stored into a fn-local 1-slot container to its STORE site.
    // The store is a consuming node in the same sense a `Return` is: it takes a
    // counted reference of its own, so the producer's claim is discharged there
    // and nowhere later. The pin has to be explicit rather than left to the
    // structural last use, because ANF names the stored value in a `let` NESTED
    // inside the assign — releasing at that inner node would free the value
    // before `lower_assign` increfs and stores it. The lowerer emits a node's
    // decrefs after the node, so at the assign the release lands behind both the
    // store's retain and the displaced prior's drop
    // (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
    // containers").
    for (lu, value_regions) in &cell_store_pins {
        info.region_data
            .pin_all_to(value_regions.iter().copied(), *lu, porder);
    }

    // Extend each returned value's region `decref_point` to its `Return`
    // node. The lowerer emits the node's `IncrefValueRegion` before the
    // node's own `emit_decrefs_for`; pinning `decref_point` here guarantees a
    // freshly-allocated result region's `DecrefRegion` fires *after* the
    // retain (so the result survives its callee-side release and is
    // handed back with one owning reference). For a pass-through arg the
    // region is the callee's phantom scope (guard-suppressed) and this
    // is inert.
    //
    // A value a fn-local 1-slot container holds is exempt on the same terms an
    // uncounted read of one is, and against the same `cell_drop_point`: the
    // producer's claim is discharged at the store, and from there the CELL's
    // counted reference is what carries the value — across the `Return` too,
    // where the mint pays for the caller's copy and the content drop (emitted
    // after that mint, at the same node) releases the cell's. So where the cell
    // drops the value at or after the return, the extension buys nothing and
    // costs the store-site pin: one release at the `Return` names whatever the
    // producer's ANF slot holds LAST, stranding every earlier value a loop
    // stored. Where the cell drops it EARLIER — the value reaches the return
    // through some other name, or through a tail branch the cell's own last
    // access precedes — the producer's reference is the return's only
    // protection and the extension stands (docs/impl/region/bindings.md § "A
    // `Return` is a reader of the cell's content").
    for (return_id, regions) in return_sites {
        info.region_data.pin_all_to(
            regions.iter().copied().filter(|r| {
                cell_drop_point
                    .get(r)
                    .is_none_or(|&drop| porder.is_after(*return_id, drop))
            }),
            *return_id,
            porder,
        );
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
        info.region_data
            .pin_all_to(regions.iter().copied(), *destructure_id, porder);
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
        info.region_data
            .pin_all_to(regions.iter().copied(), lu, porder);
    }

    // Re-anchor every release that landed inside a branch arm onto the branch.
    // Reads each region's FINAL `decref_point`, so it follows every extension
    // above; it only moves a release LATER, so the break window below still sees
    // (and can re-anchor) anything it leaves inside a block's skipped window.
    pin_branch_arm_releases(
        info,
        hir,
        du,
        order,
        last_use,
        inference_binding_regions,
        frame_replacing_tail_calls,
    );

    // Reads each region's FINAL `decref_point` to decide whether the break jumps
    // over it, so every extension above must already have landed.
    pin_break_skipped_releases(info, hir, order, last_use, break_skip_blocks);

    // Runs after the break window, and so after every pass that can place a
    // release: what it clamps against is where the value releases FINALLY land.
    pin_cell_release_after_routed_releases(
        info,
        order,
        &iter_scopes,
        inference_binding_regions,
        binder_init_sites,
    );
}
