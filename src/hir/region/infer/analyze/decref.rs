//! `decref_point` population passes.
//!
//! For each region `r`, `decref_point` is the structurally-latest program point
//! at which any value resolved to `r` is last used. These passes seed it from
//! per-HirId last-use analysis, then extend it through binding chains, hoist
//! env-cell releases past loops, and pin returned/destructured values to their
//! consuming node. Extracted verbatim from `analyze_regions_with`; the inline
//! comments explain the WHY of each pass.

// `super` is `hir::region::infer::analyze`; `super::super` reaches the sibling
// `hir::regions` items the original block saw through `use super::*`.
use super::super::arms::{branch_arms, ArmSet};
use super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::liveness::LastUseInfo;
use crate::hir::region::{PinDecref, ProgramOrder};

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

    // Every iterative scope with its post-order subtree interval `[low, order]`,
    // so containment of a HirId is an interval test. Computed once for the three
    // cell passes that read it — the fn-local container's demise hoist, the env
    // cell's once-per-activation hoist, and the clamp that follows the value
    // releases routed through an env cell — and skipped entirely when the unit
    // has no cell of either kind.
    let iter_scopes: Vec<(HirId, u32, u32)> =
        if info.cell_containers.is_empty() && info.cell_release_regions.is_empty() {
            Vec::new()
        } else {
            let low = compute_subtree_low(hir, order);
            let mut scopes = Vec::new();
            collect_iter_scopes(hir, order, &low, &mut scopes);
            scopes
        };

    // ── The fn-local 1-slot container's content drop ──────────────────────
    // The cell's own reference to its current content dies at its last access —
    // the latest of its reads and its writes — with one hoist: a cell CARRIED
    // ACROSS a loop is re-pointed every iteration, so a drop inside the body
    // would free the content the next iteration reads. Such a cell is a loop
    // PARAMETER, i.e. its scope node is the loop itself, so hoisting to that
    // node lands the one drop after the loop — where the lowerer emits the
    // loop's own releases. A cell bound INSIDE a loop body has a body scope
    // node instead, so it is not hoisted and drops once per iteration, matching
    // its per-iteration mint. And a loop's parameters stay readable past the
    // loop (the `(while … (assign acc …)) acc` idiom), which is why the hoist
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
            let latest = du
                .uses
                .get(b)
                .into_iter()
                .flat_map(|v| v.iter())
                .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
                .chain(c.stores.sites())
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
    // loop-independent. See docs/impl/region/bindings.md "Env cells in loops:
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

/// Clamp each env cell's box release to at-or-after every release routed THROUGH
/// that cell (docs/impl/region/bindings.md § "A cell's release lands at or after
/// every release routed through that cell").
///
/// A captured binding's init value and the box that holds it are addressed by one
/// env index. The value's `DecrefValueRegion` loads the box RAW and lets
/// `result_region_of` unwrap it to the content, so it READS the box's page; the
/// box's `DecrefCellRegion` frees that page. Emitted in the other order, the
/// unwrap reads memory the free reclaimed.
///
/// At one `decref_point` the release order already states this — a value release
/// that unwraps a cell reads deepest and sorts ahead of the cell release that
/// frees the page (`lir::lower::Lowerer::order_releases`,
/// docs/impl/region/rules.md Rule 4). Across two points nothing does, and the two
/// points diverge by construction: both regions
/// ride the binding's uses through the binding-chain extension, then the VALUE
/// region takes a second, later bound from its allocation site's last use, which
/// follows the binder's own value out to whatever consumes it. The cell region is
/// a phantom placeholder with no allocation site, so it keeps the binding-use
/// bound alone and the box is freed at the capture.
///
/// The one region a value route reads through the cell is the region the
/// binding's own binder ALLOCATED — the entry `record_region_slot` makes against
/// the binder's slot, which for an env-celled binding is the env index
/// (`lir::lower::binding::define`). A region the binding merely NAMES records no
/// slot and takes the release-by-id route, which reads no page: `(def @c n)`
/// names its parameter's phantom region and allocates nothing, so its box owes
/// that region's release nothing. A binder whose init allocates nothing, a
/// binding two binders claim, and a captured-reassigned binding (whose init
/// reference is dropped off its own register rather than through the cell) are
/// all absent from the route for the same reason, and are left alone here.
///
/// The clamp is a maximum like every other pin, so it can only move the box
/// release later; the point it produces is then taken out of any enclosing loop,
/// because an env cell is minted once per activation and its release must fire
/// once per activation whatever drew it inward.
fn pin_cell_release_after_routed_releases(
    info: &mut RegionInfo,
    order: &HashMap<HirId, u32>,
    iter_scopes: &[(HirId, u32, u32)],
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) {
    if info.cell_release_regions.is_empty() {
        return;
    }
    let porder = ProgramOrder::new(order);
    // Staged: the scan reads one region's `decref_point` while the pin writes
    // another's, which one pass over `region_data` cannot borrow.
    let mut pins: Vec<(Region, HirId)> = Vec::new();
    for (b, regions) in inference_binding_regions {
        let cells: Vec<Region> = regions
            .iter()
            .copied()
            .filter(|r| info.cell_release_regions.contains(r))
            .collect();
        if cells.is_empty() || info.captured_reassigned_bindings.contains(b) {
            continue;
        }
        let Some(&Some(init)) = binder_init_sites.get(b) else {
            continue;
        };
        let Some(routed) = info.alloc_region.get(&init).copied() else {
            continue;
        };
        let Some(at) = info.region_data.get(&routed).map(|d| d.decref_point) else {
            continue;
        };
        let at = post_loop_placement(at, iter_scopes, order).unwrap_or(at);
        for c in cells {
            pins.push((c, at));
        }
    }
    for (cell, at) in pins {
        info.region_data.pin_to(cell, at, porder);
    }
}

/// The node an env cell's release takes instead of `at`, so that it fires once
/// per activation rather than once per iteration: the OUTERMOST `While`/`Loop`
/// enclosing `at`, which the lowerer emits after the loop. `None` when `at` is in
/// no loop, or is already at or past the enclosing loop's own node — a release
/// anchored at the loop node already runs once per execution of the loop.
///
/// An ancestor carries the largest post-order index, so the outermost enclosing
/// scope is the containing interval with the greatest high end.
fn post_loop_placement(
    at: HirId,
    iter_scopes: &[(HirId, u32, u32)],
    order: &HashMap<HirId, u32>,
) -> Option<HirId> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let at_ord = ord(at);
    iter_scopes
        .iter()
        .filter(|&&(_, lo, hi)| lo <= at_ord && at_ord <= hi)
        .max_by_key(|&&(_, _, hi)| hi)
        .map(|&(id, _, _)| id)
        .filter(|&id| ord(id) > at_ord)
}

/// Re-anchor a release that landed inside one arm of a branch onto the branch.
///
/// A region's `decref_point` is the structurally-latest of its uses. When several
/// arms use it, "latest" resolves to a node inside ONE arm — and arms are
/// mutually exclusive, so every execution taking a different arm emits no release
/// at all and holds the whole region (plus every member its free cascade would
/// reclaim) to fiber teardown. "Latest across the arms" is not a point any single
/// execution passes through.
///
/// The point every execution does pass through is `last_use[branch]` — the node
/// consuming the branch's value, or the branch itself when nothing does — whose
/// decrefs the lowerer emits after the merge label. Moving the release there is a
/// placement argument, the one the break window makes: one release still, landing
/// later (docs/impl/region/mechanism.md § "A release inside one arm is not a
/// release on the other arms").
///
/// Placement is enough only where this frame **holds the region alone** —
/// the release fires on arms where none did before, and the other holder within
/// reach is an uncounted borrow in a parked frame. That is escape's question, and
/// the admission below is its answer; everything escape cannot clear keeps the
/// baseline and the counted per-arm routes.
///
/// The **return** facet is no refusal here. The merge is a point in this frame, so
/// every mint the taken arm ran has already fired there; and a replica ahead of an
/// arm's `TailCall`, which does precede its callee's mint, owes no edge either — a
/// callee reaches a value this frame owns as an operand or through its captured
/// environment and by no other route, so it either counts the region or cannot mint
/// against it (region/mechanism.md § "The callee's return mint, and why the point
/// owes it nothing").
///
/// Once a region's `decref_point` leaves the arms, `region::infer::compensate` no
/// longer finds it inside one, so neither of its per-arm routes fires for that
/// region — the single anchored release is what they were approximating.
///
/// Two boundaries bound the window, the same two the break window carries, and
/// both are about how many times a release runs: a `While`/`Loop` nested in the
/// branch and holding the `decref_point` (its body re-allocates per iteration, so
/// one release cannot cover N) and a `Lambda` holding it (its releases run in
/// another activation, against another frame's slots). Each is the scope's BODY,
/// never the scope's own node — a release anchored at the loop node already runs
/// once per execution of the loop.
///
/// An arm that leaves through a **frame-replacing** tail call does not arrive at
/// the merge, so the anchor alone does not cover it — the frame-exit relocation
/// does, replicating the anchored release ahead of that arm's `TailCall` or
/// leaving it to the callee that took the argument over. That replica exists only
/// for a value-routed release, so such a branch narrows the window to the regions
/// a value route can NAME (`value_routed`, `region::infer::escape`) instead of
/// declining whole; everything else keeps its in-arm release and
/// `region::infer::compensate`'s counted routes. A tail call to a *native* pushes
/// no frame and falls through, which is why the callee kind decides it
/// (`frame_replacing_tail_calls`) and not `is_tail`.
///
/// The exemption the relocation already reads (`TailExitHoist::exempt`) decides one
/// more refusal, because its two halves owe different things. An **argument** the
/// call names is the ownership move — the callee's owned-parameter release runs in
/// place of the copy the arm left in its dead block — so the anchor may take that
/// release away. The **callee's own** region has no such release standing in for
/// it: the deferred callee channel does, and that channel is keyed on where the
/// release sits, so anchoring it at the merge leaves the exiting arm with nothing
/// (region/mechanism.md § "What the exemption keeps, a channel must still run").
///
/// The region must also be **live-in** — every allocation and holder-definition
/// site outside the branch's subtree — so a value born inside an arm keeps its
/// in-arm release and the window only moves what the branch received.
#[allow(clippy::too_many_arguments)]
fn pin_branch_arm_releases(
    info: &mut RegionInfo,
    hir: &Hir,
    du: &DefUseBuilder,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    frame_replacing_tail_calls: &rustc_hash::FxHashSet<HirId>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let low = compute_subtree_low(hir, order);
    let mut scopes = BranchWindowScopes::default();
    collect_branch_scopes(
        hir,
        info,
        order,
        &low,
        frame_replacing_tail_calls,
        &mut scopes,
    );
    if scopes.branches.is_empty() {
        return;
    }
    // Inner branches first: a release hoisted to an inner branch's anchor is
    // still inside the enclosing arm, so the outer branch can carry it the rest
    // of the way. Post-order indexes a child below its parent, so ascending
    // `node_hi` is exactly innermost-outward.
    scopes.branches.sort_by_key(|b| b.node_hi);

    // The live-in premise's anchors: every allocation site of each region, and the
    // definition site of every holder binding that could be the release's ROUTE.
    // `record_region_slot` keys `region_to_slot` on a region's ALLOCATION site, so
    // the slot a value-routed release loads belongs to the binding whose init
    // allocated the region — or, for a region no site in this body allocates, to
    // the parameter the lambda prologue recorded. An ALIAS binder, whose init
    // merely names another binding, records no slot and can never be the route, so
    // its definition site anchors nothing: an arm that introduces one has not given
    // birth to the value and the branch still received it.
    let aliases = alias_binders(hir);
    let mut region_anchors: HashMap<Region, Vec<u32>> = HashMap::new();
    for (b, regions) in inference_binding_regions {
        if aliases.contains(b) {
            continue;
        }
        if let Some(&d) = du.def_site.get(b) {
            for &r in regions {
                region_anchors.entry(r).or_default().push(ord(d));
            }
        }
    }
    for (&alloc_id, &r) in &info.alloc_region {
        region_anchors.entry(r).or_default().push(ord(alloc_id));
    }
    // Only regions whose `decref_point` lands inside the branch's arms can move,
    // so index the regions by post-order `decref_point` once and let each branch
    // binary-search the arm-interval slices. A move writes the region's new ord
    // straight into the index, which leaves it unsorted, so a branch that moved
    // anything re-sorts before the next one runs — later branches, processed
    // innermost-outward, then search an ordered index and see the post-move values.
    let mut by_dord: Vec<(u32, Region)> = info
        .region_data
        .iter()
        .map(|(&r, d)| (ord(d.decref_point), r))
        .collect();
    by_dord.sort_unstable_by_key(|&(o, _)| o);

    // ── The admission: the frame must be the region's only holder ────────────
    //
    // The anchor is a PLACEMENT argument — one release, moved later — and
    // placement alone is enough only where this frame holds the region's only
    // reference. On the arms the window newly covers the release fires where none
    // did before, so any *other* holder it drops to zero is an over-free; and the
    // reachable other holder is an uncounted borrow in a frame that is PARKED when
    // the release runs, which the resume's uncounted-borrow check detonates on
    // (region/generations.md). No premise about arm structure discharges that.
    //
    // Escape answers exactly this question, and it is the sole authority for it: a
    // value that does not leave its activation by any facet but RETURN is reachable
    // only through this frame's slots for as long as the frame lives, so the frame
    // is the only holder at the merge. Every holder must clear that reading (an
    // aliased region is only as local as its loosest holder), the region must have
    // one (an unheld region offers nothing to judge), and the fiber frontier's
    // atomless site half is refused too, since no binding names it.
    // One predicate, shared with the lowerer's frame-exit release, and computed once
    // by `analyze_regions_with` before any of these passes run — both mechanisms make
    // a release fire where none fired before, so both owe escape the same count
    // argument, and reading it from `RegionInfo` is what keeps them one answer rather
    // than two. A MUTATED route is refused for the reason
    // `region::infer::compensate` refuses it as a release route — a slot repointed
    // between the arm and the anchor frees whatever it holds THEN — and that is asked
    // of the region's own route binding, not of every binding that names the value.
    // Lexical capture is NOT refused: a closure's hold on what it captures is the
    // funnel's counted (or the forest's owning) edge, never the uncounted borrow this
    // admission guards against, and capture by a closure that *escapes* is already
    // one of escape's facets.
    //
    // The **return** facet is no refusal at either placement. At the merge the
    // release follows whatever mint the taken arm already ran (§ "The return
    // frontier is per-path"); at an arm's replica, which does precede its callee's
    // mint, the callee reaches a value this frame owns as an operand or through its
    // captured environment and by no other route — so it either holds a counted edge
    // across the gap or cannot mint against the region at all (mechanism.md § "The
    // callee's return mint, and why the point owes it nothing").
    let frame_held = info.frame_held_regions.clone();

    // Snapshotted for the frame-exit narrowing below, which reads it while
    // `region_data` is borrowed mutably (`RegionInfo::value_routed_regions`).
    let value_routed = info.value_routed_regions.clone();

    // Regions whose release belongs to another mechanism: moving their
    // `decref_point` would move a release that mechanism, not this one, emits.
    let excluded: rustc_hash::FxHashSet<Region> = info
        .region_data
        .keys()
        .copied()
        .filter(|&r| {
            info.suppressed_decref_regions.contains(&r)
                || info.owned_group_members.contains(&r)
                || info.cell_release_regions.contains(&r)
                || info.mutated_binding_value_regions.contains(&r)
                || info.merged_root(r) != r
                || !frame_held.contains(&r)
        })
        .collect();

    for br in &scopes.branches {
        // A nested lambda's own frame exits belong to that lambda, not here.
        let inner_lambdas: Vec<(u32, u32)> = scopes
            .lambdas
            .iter()
            .copied()
            .filter(|&(lo, hi)| br.node_lo <= lo && hi < br.node_hi)
            .collect();
        // The arms that leave through a callee instead of arriving at the merge.
        // The anchor still covers the arms that fall through, and the frame-exit
        // relocation covers the rest — but only for a release it can REPLICATE,
        // which is the value route alone. So their presence narrows the window to
        // the regions a value route can name here rather than declining the branch
        // whole; every other region keeps its in-arm release and compensation's
        // counted routes. `value_routed` asks the question the emitter asks
        // (`lir::lower::regiondecref::value_release_slot`) rather than reading the
        // region's class: releasing by id is the lowerer's DEFAULT, taken wherever a
        // single point covers every path, so a region a binder recorded a slot for
        // takes the value route as soon as a point admits it (mechanism.md § "A
        // release the relocation replicates names a VALUE, and a binder's slot
        // supplies that name").
        let arm_exits: Vec<&FrameExit> = scopes
            .frame_exits
            .iter()
            .filter(|e| {
                e.at >= br.node_lo
                    && e.at <= br.node_hi
                    && !inner_lambdas
                        .iter()
                        .any(|&(lo, hi)| lo <= e.at && e.at <= hi)
            })
            .collect();
        let arm_exits_frame = !arm_exits.is_empty();
        let anchor = last_use.get(&br.id).copied().unwrap_or(br.id);
        let anchor_ord = ord(anchor);
        // Only the barriers nested INSIDE this branch matter: one enclosing the
        // branch encloses the anchor too, so it constrains nothing here.
        let inner_barriers: Vec<(u32, u32)> = scopes
            .barriers
            .iter()
            .copied()
            .filter(|&(lo, hi)| br.node_lo <= lo && hi < br.node_hi)
            .collect();
        // The exemption below only asks whether r is in ANY exiting arm's callee
        // set, so the per-arm sets are merged once per branch and the per-region
        // test is an O(1) membership lookup in that union.
        let callee_union: rustc_hash::FxHashSet<Region> = arm_exits
            .iter()
            .flat_map(|e| e.callee.iter().copied())
            .collect();
        // Merge the half-open barrier intervals into one ordered disjoint list,
        // then answer `any(lo <= dord < hi)` with a binary search. Disjointness is
        // what the search rests on: among disjoint intervals only the LAST whose
        // `lo <= dord` can hold dord, because every earlier one ends at or before
        // that `lo`. Overlapping intervals merge; adjacent ones ([1,5) and [5,8))
        // are left alone, since they are already disjoint — 5 is in the second.
        let mut sorted_barriers = inner_barriers;
        sorted_barriers.sort_unstable_by_key(|&(lo, _)| lo);
        let mut merged_barriers: Vec<(u32, u32)> = Vec::new();
        for &(lo, hi) in &sorted_barriers {
            if let Some(last) = merged_barriers.last_mut() {
                if lo < last.1 {
                    if hi > last.1 {
                        last.1 = hi;
                    }
                    continue;
                }
            }
            merged_barriers.push((lo, hi));
        }
        let in_barrier = |dord: u32| -> bool {
            let i = merged_barriers.partition_point(|&(lo, _)| lo <= dord);
            i > 0 && merged_barriers[i - 1].1 > dord
        };
        // Merge the arms' closed intervals (`lo <= dord <= hi`) so each region in
        // the branch's union is scanned once, then binary-search the `by_dord`
        // index for each merged interval.
        let mut arm_ivs: Vec<(u32, u32)> = br.arms.iter().map(|a| (a.lo, a.hi)).collect();
        arm_ivs.sort_unstable_by_key(|&(lo, _)| lo);
        let mut merged_arms: Vec<(u32, u32)> = Vec::new();
        for (lo, hi) in arm_ivs {
            if let Some(last) = merged_arms.last_mut() {
                if lo <= last.1 {
                    if hi > last.1 {
                        last.1 = hi;
                    }
                    continue;
                }
            }
            merged_arms.push((lo, hi));
        }
        // Snapshot the branch's candidates before any move: the inner loop mutates
        // `by_dord` in place (a move raises the entry's ord), so iterating the live
        // slice would shift not-yet-visited entries across the fixed `end` boundary
        // and admit regions whose ord falls OUTSIDE the arm union. Collecting the
        // union's entries once — each region appears in exactly one merged interval —
        // matches the original per-region pass, which read every `decref_point`
        // before mutating any of them.
        let mut candidates: Vec<(u32, Region, usize)> = Vec::new();
        for &(lo, hi) in &merged_arms {
            let start = by_dord.partition_point(|&(o, _)| o < lo);
            let end = by_dord.partition_point(|&(o, _)| o <= hi);
            for (rel, &(o, r)) in by_dord[start..end].iter().enumerate() {
                candidates.push((o, r, start + rel));
            }
        }
        let mut moved = false;
        for &(dord, r, idx) in &candidates {
            if excluded.contains(&r) {
                continue;
            }
            if arm_exits_frame && !value_routed.contains(&r) {
                continue;
            }
            // A region the exiting arm's call names as its CALLEE is exempt from the
            // relocation, so no replica reaches that arm — and what stands in for the
            // release there is the deferred callee channel, which is keyed on where
            // the release SITS (`deferred_release_slot`; mechanism.md § "What the
            // exemption keeps, a channel must still run"). Moving it to the merge
            // takes it out of that channel's reach and leaves the arm with nothing,
            // so such a region keeps its in-arm release. An ARGUMENT's exemption is
            // the opposite story — the release the arm never runs IS the ownership
            // move, and the callee's owned-parameter release consumes it — so an
            // argument is no refusal here (rules.md Rule 5).
            if callee_union.contains(&r) {
                continue;
            }
            if dord >= anchor_ord {
                continue;
            }
            // A boundary is the scope's BODY, not the scope's own node: the
            // lowerer emits a node's releases after it finishes lowering that
            // node, so a `decref_point` AT the `While`/`Loop` lands after the loop
            // and runs once per execution of it — the count the merge label is
            // reached with. A `Lambda` node reads the same way; only its body runs
            // in another activation. The interval is therefore half-open on the
            // high end, which is what admits a live-in region a nested loop merely
            // READS: the loop-node extension anchors every such read at the loop
            // node (`hir/liveness/lastuse`), so the closed reading would leave the
            // branch's only release under the looping arm.
            if in_barrier(dord) {
                continue;
            }
            let live_in = region_anchors
                .get(&r)
                .is_some_and(|a| a.iter().all(|&o| o < br.node_lo || o > br.node_hi));
            if !live_in {
                continue;
            }
            // The window moves only where the release is EMITTED. `lifetime_point`
            // stays at the structural last use, so the ownership and merge cuts —
            // whose post-dominance obligations are lifetime questions — keep
            // reading the region's real lifetime and not this anchor
            // (`RegionData::lifetime_point`).
            info.region_data.get_mut(&r).unwrap().decref_point = anchor;
            by_dord[idx].0 = anchor_ord;
            moved = true;
        }
        // A move raises a region's `decref_point` in place and so unsorts the
        // index; restore the order so the next branch's interval searches hold.
        // Most branches move nothing, and those owe no sort at all.
        if moved {
            by_dord.sort_unstable_by_key(|&(o, _)| o);
        }
    }
}

/// Every binding whose initializer merely NAMES another binding — an alias.
///
/// Such a binder allocates nothing, so `record_region_slot` records no slot for
/// it (`alloc_region` has no entry at a `Var` init) and no value-routed release
/// can ever load its slot. It is therefore not an anchor for the branch-arm
/// window's live-in premise: an arm that introduces one has not given birth to the
/// value, and the region the branch received is still the branch's to release
/// (region/mechanism.md § "The boundaries").
///
/// The descent looks through `DerefCell`, the wrapper functionalization puts
/// around a read of a `needs_capture` binding, for the same reason the tail
/// callee's resolution does: matching the bare `Var` alone would miss most real
/// aliases.
///
/// One alias binder does own a slot, and the allocation half of the anchor set is
/// what keeps it honest: a whole-value read of a 1-slot container mints a
/// placeholder region AT the read (`counted_cell_read_sites`), which is an
/// `alloc_region` entry at this very init — so the reader's own placeholder still
/// anchors on the reader, while the container's region, which the reader only
/// names, does not.
fn alias_binders(hir: &Hir) -> rustc_hash::FxHashSet<Binding> {
    fn names_a_binding(init: &Hir) -> bool {
        match &init.kind {
            HirKind::Var(_) => true,
            HirKind::DerefCell { cell } => names_a_binding(cell),
            _ => false,
        }
    }
    fn walk(h: &Hir, out: &mut rustc_hash::FxHashSet<Binding>) {
        match &h.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, init) in bindings {
                    if names_a_binding(init) {
                        out.insert(*b);
                    }
                }
            }
            HirKind::Define { binding, value, .. } if names_a_binding(value) => {
                out.insert(*binding);
            }
            _ => {}
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = rustc_hash::FxHashSet::default();
    walk(hir, &mut out);
    out
}

/// The structural facts [`pin_branch_arm_releases`] reads off one compilation
/// unit, all as post-order indices so containment is an interval test.
#[derive(Default)]
struct BranchWindowScopes {
    /// Every branch, with its own and its arms' subtree intervals
    /// ([`crate::hir::region::infer::arms`]). An `If` is a two-armed branch —
    /// every premise here is stated over one arm and its siblings, never over the
    /// branch's kind or arity — and a short-circuiting form contributes the arms
    /// of its nested-`If` equivalent, one entry per conditional level. All the
    /// entries of one form name that form's node, so they share the anchor and
    /// the live-in premise; the first to fire moves the release to the anchor and
    /// the rest then find it at or past that point.
    branches: Vec<ArmSet>,
    /// Subtree intervals of the scopes a release may not be hoisted OUT of: an
    /// iterative scope (`While`/`Loop`, whose body re-allocates per iteration)
    /// and a `Lambda` (whose body runs in its own activation, against its own
    /// frame's slots).
    barriers: Vec<(u32, u32)>,
    /// Subtree intervals of the `Lambda`s alone — the frame boundary, which says
    /// whose exits a `frame_exits` entry belongs to.
    lambdas: Vec<(u32, u32)>,
    /// The tail calls that may replace the frame. One inside a branch means its
    /// merge label is not a point every arm reaches, so the branch anchors only the
    /// releases the relocation can replicate into that arm.
    ///
    /// Narrower than [`BreakWindowScopes::frame_exits`], which counts every
    /// `Return` and every tail `Call`: a functionalized `Return` inside an arm
    /// stores the branch's result and jumps to the merge rather than leaving, and
    /// a tail call to a *native* falls through to it. Only a callee that can
    /// replace the frame actually skips the merge, and reading the coarser set
    /// would narrow every native-tail dispatch arm for nothing.
    frame_exits: Vec<FrameExit>,
}

/// One frame-replacing tail call, as the branch-arm window reads it.
struct FrameExit {
    /// Post-order index of the call, so containment in a branch or in a nested
    /// lambda is an interval test.
    at: u32,
    /// The regions the call names as its **callee** — the closure region it reaches
    /// the callee through. This is the half of `TailExitHoist::exempt` no
    /// owned-parameter release stands in for: the relocation leaves such a release
    /// where it sits, and the deferred callee channel runs it from there
    /// (`deferred_release_slot`, `TailExitHoist::exempt`; mechanism.md § "What the
    /// exemption keeps, a channel must still run").
    callee: rustc_hash::FxHashSet<Region>,
}

/// Collect [`BranchWindowScopes`] over the whole tree in one walk.
fn collect_branch_scopes(
    hir: &Hir,
    info: &RegionInfo,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    frame_replacing_tail_calls: &rustc_hash::FxHashSet<HirId>,
    out: &mut BranchWindowScopes,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = |id: HirId| low.get(&id).copied().unwrap_or(0);
    match &hir.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => {
            out.barriers.push((lo(hir.id), ord(hir.id)))
        }
        HirKind::Lambda { .. } => {
            out.barriers.push((lo(hir.id), ord(hir.id)));
            out.lambdas.push((lo(hir.id), ord(hir.id)));
        }
        HirKind::Call { func, .. } if frame_replacing_tail_calls.contains(&hir.id) => {
            // The callee half of `TailExitHoist::exempt`, read off the same
            // `operand_value_regions` the lowerer reads so the two cannot disagree
            // about which releases the relocation will decline to replicate.
            let mut callee = rustc_hash::FxHashSet::default();
            info.operand_value_regions(func, &mut callee);
            out.frame_exits.push(FrameExit {
                at: ord(hir.id),
                callee,
            });
        }
        _ => {}
    }
    out.branches.extend(branch_arms(hir, order, low));
    hir.for_each_child(|c| {
        collect_branch_scopes(c, info, order, low, frame_replacing_tail_calls, out)
    });
}

/// Re-anchor every release a `break` jumps over onto the block it leaves.
///
/// The pin above covers the value the break CARRIES. Every other region whose
/// release sits in the same window — inside the block's body, at or after the
/// break site, before the exit label — is passed over by the identical jump, and
/// has no consumer to be handed to: the release is emitted into unreachable code
/// and the region is held to fiber teardown. Re-anchoring to `last_use[block]` —
/// the first point both the break path and the fall-through path reach, the same
/// anchor the broken value takes — is enough, and needs no release at the break
/// site: moving a release LATER can only over-keep (docs/impl/region/mechanism.md
/// § "A release the break jumps over is not a release").
///
/// The window is read off the structural order: a node's releases are skipped
/// exactly when its post-order index is at or above the break's. That covers the
/// break node itself (`lower_break` terminates with the jump, so its own decrefs
/// land in the dead block after it) and every enclosing `let`/`begin`, whose
/// releases the lowerer emits after the body.
///
/// Two scopes bound the window, both because a release inside them must run a
/// different number of times than the block's exit label is reached: a nested
/// `While`/`Loop` (a loop-body value is re-allocated per iteration, so one
/// release cannot cover N) and a nested `Lambda` (its releases run in another
/// activation, against another frame's slots). Inside either, the release stays
/// where it is — still skipped on the break path, an over-keep bounded by one
/// iteration / one call, never a mis-free.
///
/// A third condition guards the anchor itself: the exit label has to be a point
/// every path actually **reaches**. A frame-replacing exit inside the body — a
/// `Return`, or a `Call` in tail position, which the lowerer emits as `TailCall`
/// — leaves through the callee instead of arriving there, so a release moved to
/// the anchor would be dead on exactly the path that used to run it (trading one
/// leak for another). Such a block declines the window whole.
fn pin_break_skipped_releases(
    info: &mut RegionInfo,
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
    break_skip_blocks: &[(HirId, Vec<HirId>)],
) {
    if break_skip_blocks.is_empty() {
        return;
    }
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let low = compute_subtree_low(hir, order);
    let mut scopes = BreakWindowScopes::default();
    collect_window_scopes(hir, order, &low, &mut scopes);

    for (block_id, sites) in break_skip_blocks {
        // The window opens at the EARLIEST targeting break: a release after it is
        // skipped whenever that break fires, and the pin has to hold for every
        // path through the block, not just the last one.
        let Some(first_break) = sites.iter().map(|s| ord(*s)).min() else {
            continue;
        };
        let block_lo = low.get(block_id).copied().unwrap_or(0);
        let block_hi = ord(*block_id);
        let anchor = last_use.get(block_id).copied().unwrap_or(*block_id);
        let anchor_ord = ord(anchor);
        // Only the barriers nested INSIDE this block matter: one enclosing the
        // block encloses the anchor too, so it constrains nothing here.
        let inner: Vec<(u32, u32)> = scopes
            .barriers
            .iter()
            .copied()
            .filter(|&(lo, hi)| block_lo <= lo && hi < block_hi)
            .collect();
        // Does anything in the window leave the frame before the exit label? A
        // nested lambda's own exits belong to that lambda, not to this block.
        let lambdas: Vec<(u32, u32)> = scopes
            .lambdas
            .iter()
            .copied()
            .filter(|&(lo, hi)| block_lo <= lo && hi < block_hi)
            .collect();
        if scopes.frame_exits.iter().any(|&e| {
            e >= first_break && e <= block_hi && !lambdas.iter().any(|&(lo, hi)| lo <= e && e <= hi)
        }) {
            continue;
        }
        for d in info.region_data.values_mut() {
            let dord = ord(d.decref_point);
            // In the body (the block node's own decrefs are already at the
            // anchor), at or after the break, and not already later than it.
            if dord < first_break || dord < block_lo || dord >= block_hi || dord >= anchor_ord {
                continue;
            }
            if inner.iter().any(|&(lo, hi)| lo <= dord && dord <= hi) {
                continue;
            }
            d.decref_point = anchor;
        }
    }
}

/// The structural facts `pin_break_skipped_releases` reads off one compilation
/// unit, all as post-order indices so containment is an interval test.
#[derive(Default)]
struct BreakWindowScopes {
    /// Subtree intervals of the scopes a release may not be hoisted OUT of: an
    /// iterative scope (`While`/`Loop`, whose body re-allocates per iteration)
    /// and a `Lambda` (whose body runs in its own activation, against its own
    /// frame's slots).
    barriers: Vec<(u32, u32)>,
    /// Subtree intervals of the `Lambda`s alone — the frame boundary, which says
    /// whose exits a `frame_exits` entry belongs to.
    lambdas: Vec<(u32, u32)>,
    /// Nodes that leave the enclosing frame instead of falling through to it: a
    /// `Return`, and a `Call` in tail position (lowered as a frame-replacing
    /// `TailCall`). One in a block's window means the block's exit label is not
    /// a point every path reaches.
    frame_exits: Vec<u32>,
}

/// Collect [`BreakWindowScopes`] over the whole tree in one walk.
fn collect_window_scopes(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut BreakWindowScopes,
) {
    let lo = low.get(&hir.id).copied().unwrap_or(0);
    let hi = order.get(&hir.id).copied().unwrap_or(0);
    match &hir.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => out.barriers.push((lo, hi)),
        HirKind::Lambda { .. } => {
            out.barriers.push((lo, hi));
            out.lambdas.push((lo, hi));
        }
        HirKind::Return { .. } => out.frame_exits.push(hi),
        HirKind::Call { is_tail: true, .. } => out.frame_exits.push(hi),
        _ => {}
    }
    hir.for_each_child(|c| collect_window_scopes(c, order, low, out));
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
