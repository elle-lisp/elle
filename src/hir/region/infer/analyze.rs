// audited: 2026-09-29
//! The region-inference pipeline: the walk, then every post-pass that decides
//! where a release lands.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/mechanism.md
//!
//! The order is the point: each pass reads answers the ones before it settled.

use super::letrec::classify_letrec_callees;
use super::*;

// The post-`build_info` phases live in submodules; the root orchestrates them.
mod adopt;
mod decref;
mod reassign;

// ── Public API ─────────────────────────────────────────────────────

/// Run region inference on a functionalized HIR tree.
pub fn analyze_regions(hir: &Hir, arena: &BindingArena) -> RegionInfo {
    analyze_regions_with(hir, arena, CallClassification::default())
}

/// Run region inference with call classification data.
pub fn analyze_regions_with(
    hir: &Hir,
    arena: &BindingArena,
    mut call_class: CallClassification,
) -> RegionInfo {
    // Pre-pass: classify letrec-bound lambdas
    let user_imm = classify_letrec_callees(hir, arena, &call_class);
    call_class.user_immediates = user_imm;

    // Authoritative escape facts over the canonical HIR: the one escape analysis
    // every consumer reads. The MODULE-SCOPE half of the reassign 1-slot-container
    // gate below reads its **return facet** (`binding_escapes_via_return`) for its
    // not-returned check, instead of recomputing escape from region signals.
    // Computed here, before `call_class` is moved into the inference;
    // `analyze_escape` needs the declared native effects (`call_class.effects`) for
    // its store facet, already populated.
    let escape_info = crate::hir::analyze_escape(hir, arena, &call_class);

    // The append seed and the transferred-returned-subtree cut read the call
    // classification AFTER the walk consumes it. The seed reads the declared
    // effects and return types of the natives a container meets. The cut's
    // consumer sites are gated by the declared effects (an `Immediate`-native
    // read is harmless), and the fiber symbols name its fiber face. Snapshot it
    // here, before the move into the inference.
    let transfer_call_class = call_class.clone();

    let mut ri = RegionInference::new(arena, call_class);
    // Synthetic program-root region. No Region(0) sentinel — the
    // tree uses Option<Region> for roots, so every region is real.
    let root = ri.tree.fresh_root(&mut ri.next_region);
    ri.current_region = root;
    // The entry function returns the top-level expression's value, so the program
    // tail is part of the return frontier — but that is escape's judgment
    // (`analyze_escape` seeds the top-level tail; `region::infer::escape` projects it),
    // not a fact the solver records here.
    ri.walk(hir);
    // Capture binding_regions before build_info consumes ri — used
    // below to extend decref_point through binding chains.
    let inference_binding_regions = std::mem::take(&mut ri.binding_regions);
    let return_sites = std::mem::take(&mut ri.return_sites);
    let destructure_sites = std::mem::take(&mut ri.destructure_sites);
    let break_sites = std::mem::take(&mut ri.break_sites);
    let break_skip_blocks = std::mem::take(&mut ri.break_skip_blocks);
    let frame_replacing_tail_calls = std::mem::take(&mut ri.frame_replacing_tail_calls);
    let reassigns = reassign::Reassigns {
        top_level: std::mem::take(&mut ri.top_level_reassigns),
        local: std::mem::take(&mut ri.local_reassigns),
        loop_forwarded: std::mem::take(&mut ri.loop_forwarded_params),
        binder_init_sites: std::mem::take(&mut ri.binder_init_sites),
    };
    let captured_reassigns = std::mem::take(&mut ri.captured_reassigns);
    let mut info = ri.build_info();
    // Mirror to the public surface so tests and downstream consumers can
    // inspect which source regions each binding may point into without
    // re-running the inference. Single owner; clone is cheap relative
    // to the cost of re-doing the walk.
    info.binding_source_regions = inference_binding_regions.clone();
    info.captured_reassigned_bindings = captured_reassigns;

    // Mutated-slot backstop for RE-STORABLE compiled capture cells. A mutable
    // captured binding's cell content is repointed over time, and
    // `handle_update_capture` counts it, not the 1-slot-container maps. A
    // whole-value read through the cell can still be solved to the CELL's own
    // compiled region, which a scope arm inserts into `binding_regions[b]`. That
    // region names the cell, not whatever content the cell holds at read time. A
    // static route against it, such as a coalesced return or store retain, would
    // resolve the slot against repointed content, and `AssertRegionMatches` would
    // fail on a `(deref-cell x)` tail read of a mutated `(var x …)`. Poison
    // exactly the cell regions so `coalescible_solver_region` refuses and such
    // reads stay value-resolved.
    // Keyed on `is_restorable_capture_cell`, the re-store predicate, which the
    // binding's declaration decides; `captured_reassigned_bindings` holds only
    // the bindings the walk saw written, so a declared-mutable cell with no
    // write the walk reaches would escape it. Deliberately NOT the binding's
    // full source-region set: the init value's own alloc region stays
    // coalescible — its init-drop in `store_captured_cell_init` fires while the
    // cell still holds the init (pinned by
    // `captured_reassign_init_drop_is_slot_resolved`).
    for cells in info.begin_cell_regions.values() {
        for (b, cell_region) in cells {
            if arena.get(*b).is_restorable_capture_cell() {
                info.mutated_binding_value_regions.insert(*cell_region);
            }
        }
    }

    // Def-use facts over the canonical HIR. The reassign gate, the last-use
    // analysis and the `decref_point` passes below all read them (see `decref`).
    let mut du = DefUseBuilder::new();
    du.walk(hir);

    // Explicit structural execution-order index. `decref_point` selection
    // compares these indices, never `HirId` magnitude (which ANF makes
    // meaningless — see `compute_order`). Computed here, ahead of the reassign
    // gate: the gate's feeder question is about where a store sits relative to a
    // name's scope and reads, which is the same structural order.
    let order = compute_order(hir);
    let postdom = super::postdom::PostDom::new(hir, &order);

    // ── Mutable-reassign: the cell as a 1-slot container ───────────────────
    // A reassigned mutable binding holds different values over time; no single
    // static program point names "the value's last use", so model the cell as a
    // 1-slot mutable container instead (see `reassign`). Runs before the
    // decref_point passes so the suppression/backstop sets it records are in
    // place. `du` supplies the read-alias filter, `escape_info` the return facet.
    reassign::apply_reassign_containers(
        &mut info,
        arena,
        &du,
        &inference_binding_regions,
        &reassigns,
        &escape_info,
        &postdom,
    );

    let last_use_info = compute_last_use(hir, &du.uses, &order);

    // Escape's answer to the COUNT question, projected onto regions: which regions
    // this frame holds alone for as long as it lives. Two mechanisms owe exactly
    // this admission: the branch-arm release window below, and the lowerer's
    // frame-exit release at a tail call. Both make a release fire on a path where
    // none fired before (docs/impl/region/window.md, docs/impl/region/relocate.md),
    // so it is recorded once, here. Computed before the decref passes so it reads
    // the escape facts, not any placement they go on to change.
    info.frame_held_regions = super::escape::frame_held_regions(
        &escape_info,
        arena,
        &info,
        &inference_binding_regions,
        &reassigns.binder_init_sites,
    );

    // How many arguments each frame-replacing tail call's own callee turns into
    // owned parameters, where this compilation can resolve the callee at all.
    info.tail_callee_facts = super::escape::tail_callee_facts(hir, &frame_replacing_tail_calls);

    // Which regions a value-routed release can NAME. Those are the releases the
    // frame-exit relocation can replicate into a branch arm, and so the regions the
    // branch-arm window may anchor when an arm leaves through a frame-replacing
    // callee (docs/impl/region/window.md). Recorded here, beside the other admission
    // the window owes, because the mirror of the lowerer's `region_to_slot` reads
    // `binder_init_sites` — which the walk holds and the decref passes do not.
    info.value_routed_regions =
        super::escape::value_routed_regions(arena, &info, &reassigns.binder_init_sites);

    // Populate and extend every region's `decref_point`: alloc/cell seeds,
    // binding-chain extension, env-cell loop hoist, the return/destructure/
    // break consuming-and-transferring-node pins, and the two re-anchoring
    // windows — branch-arm and break-skipped (see `decref`).
    decref::populate_decref_points(
        &mut info,
        hir,
        &du,
        &order,
        &last_use_info,
        &inference_binding_regions,
        &return_sites,
        &destructure_sites,
        &break_sites,
        &break_skip_blocks,
        &frame_replacing_tail_calls,
        &reassigns.binder_init_sites,
    );
    let last_use = &last_use_info.per_node;

    // Per-path branch compensation (`region::infer::compensate`), the counted route for
    // every region the branch-arm window above declined. A region whose single
    // `decref_point` sits inside a conditional arm is freed there on the used path
    // but leaks on the sibling arms, so add a compensating release at each dead
    // sibling arm's head. Reads the FINAL `region_data` (the decref_point post-passes
    // above) and the exclusion sets, so it runs after them. It runs before the merge
    // seeds below, so the merge-child exclusion it applies reads an empty forest.
    let branch_comp = super::compensate::compute_branch_compensation(
        hir,
        &info,
        &escape_info,
        &du,
        arena,
        &order,
        last_use,
        &return_sites,
        &reassigns.binder_init_sites,
    );
    info.branch_compensation = branch_comp.head;
    info.branch_arm_decrefs = branch_comp.tail;
    info.container_release_sites = branch_comp.container_release_sites.into_iter().collect();

    // The append seed (docs/impl/region/colocation.md): the allocations that join
    // the region of the container they are pushed into. It runs before the merge
    // seeds and the ownership forest, which leave every join region alone.
    let joins = super::join::compute_joins(hir, arena, &info, &transfer_call_class);
    info.joins = joins.sites;
    info.join_regions = joins.regions;

    // The builder-idiom merge seed (docs/impl/region/merging.md). Its lifetime gate
    // reads the final `region_data`, so it follows every decref_point post-pass
    // above. The lowerer consumes the resulting `merged_parent` forest through
    // `static_slot`'s `merged_root` canonicalization (one slot per merge tree). An
    // empty forest leaves it the identity, that is, the unmerged baseline.
    info.merged_parent = super::merge::compute_merges(hir, arena, &info, &escape_info, &order);

    // The letrec closure-cycle merge (docs/impl/region/letrec.md): a self/mutual
    // recursive closure SCC and its prebound capture cells collapse onto one arena,
    // extending the same `merged_parent` forest and riding the same `merged_root`
    // canonicalization as the builder-idiom seed. Unconditional (not flag-gated), so
    // it lands on every tier. The single `DecrefRegion` fires at the root region's
    // `decref_point`, set here to the merge's own `drop_site` (see
    // `ClosureCycleMerge::drop_site`). Runs after the builder seed (it shares the
    // map) and before the ownership pass (so `is_merged` excludes these members).
    for cm in super::merge::compute_closure_cycle_merges(hir, arena, &info, &escape_info, &order) {
        for &m in &cm.members {
            // The member set (roots included) feeds `tail_callee_defers_release`' refusal
            // and the letrec-body stranded-cycle marking: the merged arena has
            // exactly one release channel, so no other tail call may adopt it.
            info.closure_cycle_members.insert(m);
            if m != cm.root {
                info.merged_parent.insert(m, cm.root);
            }
        }
        // A NON-member body tail (a native `%add`, a redefined `+`, a foreign `g`)
        // strands the binding-scope drop past the frame-replacing `TailCall`. The
        // lowerer keys `deferred_release_slot = static_slot(root)` at each such site,
        // so the activation-completion release balances a closure callee's frame
        // replacement (docs/impl/region/letrec.md). A member tail keeps its own
        // `stranded_cycle_bindings` channel and is not recorded here.
        for &site in &cm.tail_release_sites {
            info.cycle_tail_release.insert(site, cm.root);
        }
        info.region_data
            .entry(cm.root)
            .and_modify(|d| d.extend_to(cm.drop_site))
            .or_insert(RegionData::at(cm.drop_site));
    }

    // Ownership forest: adopt edges, the transferred-returned-subtree cut, the
    // co-owned-cycle cut, and the activation-owner cut. Runs after the final
    // `region_data`, the append seed and the merge seeds above (see `adopt`).
    adopt::apply_ownership(
        &mut info,
        hir,
        &escape_info,
        arena,
        &order,
        &transfer_call_class,
    );

    // Which `Emit` sites yield a payload their own body releases nowhere, so the
    // lowerer can mint the body's missing reference there (docs/impl/region/park.md).
    // Runs after every decref_point post-pass and both merge seeds: the question is
    // where a region's release lands, and a merged child's release is its root's.
    info.borrowed_emit_payloads = super::yieldborrow::compute_borrowed_emit_payloads(hir, &info);

    // The other half of the same symmetry: which `Emit` sites receive a resume value
    // a binding's release will give back, so the lowerer can mint the reference this
    // body holds it by.
    info.unfunded_resume_values = super::yieldborrow::compute_unfunded_resume_values(hir);

    info
}
