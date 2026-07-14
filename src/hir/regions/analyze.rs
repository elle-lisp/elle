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

    // Authoritative escape facts over the canonical HIR. The reassign
    // 1-slot-container gate below reads this (its **return facet**,
    // `binding_escapes_via_return`) for the gate's not-returned check, instead of
    // recomputing escape from region signals — the one escape analysis every
    // consumer reads. Computed here, before `call_class` is moved into the
    // inference; `analyze_escape` needs the declared native effects
    // (`call_class.effects`) for its store facet, already populated.
    let escape_info = crate::hir::analyze_escape(hir, arena, &call_class);

    // The transferred-returned-subtree cut reads the call classification AFTER
    // the walk consumes it — the declared effects gate its consumer sites (an
    // `Immediate`-native read is harmless) and the fiber symbols name its fiber
    // face. Snapshot it here, before the move into the inference.
    let transfer_call_class = call_class.clone();

    let mut ri = RegionInference::new(arena, call_class);
    // Synthetic program-root region. No Region(0) sentinel — the
    // tree uses Option<Region> for roots, so every region is real.
    let root = ri.tree.fresh_root(&mut ri.next_region);
    ri.current_region = root;
    // The entry function returns the top-level expression's value, so the program
    // tail is part of the return frontier — but that is escape's judgment
    // (`analyze_escape` seeds the top-level tail; `regions::escape` projects it),
    // not a fact the solver records here.
    ri.walk(hir);
    // Capture binding_regions before build_info consumes ri — used
    // below to extend decref_point through binding chains.
    let inference_binding_regions = std::mem::take(&mut ri.binding_regions);
    let return_sites = std::mem::take(&mut ri.return_sites);
    let destructure_sites = std::mem::take(&mut ri.destructure_sites);
    let top_level_reassigns = std::mem::take(&mut ri.top_level_reassigns);
    let local_reassigns = std::mem::take(&mut ri.local_reassigns);
    let captured_reassigns = std::mem::take(&mut ri.captured_reassigns);
    let mut info = ri.build_info();
    // Mirror to the public surface so tests and downstream consumers can
    // inspect which source regions each binding may point into without
    // re-running the inference. Single owner; clone is cheap relative
    // to the cost of re-doing the walk.
    info.binding_source_regions = inference_binding_regions.clone();
    info.captured_reassigned_bindings = captured_reassigns;

    // Mutated-slot backstop for RE-STORABLE compiled capture cells. A mutable
    // captured binding's cell content is repointed over time (its RC is owned
    // by `handle_update_capture`, not the 1-slot-container maps), but a
    // whole-value read through the cell can still be solved to the CELL's own
    // compiled region (the Begin pre-pass CaptureCell insertion into
    // `binding_regions[b]`). That region names the cell, not whatever content
    // the cell holds at read time, so a static route against it — a coalesced
    // return/store retain — resolves the slot against repointed content (the
    // `AssertRegionMatches` mis-coalesce on a `(deref-cell x)` tail read of a
    // mutated `(var x …)`). Poison exactly the cell regions so
    // `coalescible_solver_region` refuses and such reads stay value-resolved.
    // Keyed on `is_restorable_capture_cell` (the re-store predicate), NOT on
    // `captured_reassigned_bindings` — the latter only sees module-scope
    // reassigns, and a `(begin (var x …) …)` single-form file reassigned from
    // inside a sibling closure is neither module-scope-classified nor
    // fn-local. Deliberately NOT the binding's full source-region set: the
    // init value's own alloc region stays coalescible — its init-drop in
    // `store_captured_cell_init` fires while the cell still holds the init
    // (pinned by `captured_reassign_init_drop_is_slot_resolved`).
    for cells in info.begin_cell_regions.values() {
        for (b, cell_region) in cells {
            if arena.get(*b).is_restorable_capture_cell() {
                info.mutated_binding_value_regions.insert(*cell_region);
            }
        }
    }

    // Populate `region_data.decref_point` from per-HirId last-use analysis.
    // For each region r, `decref_point` is the maximum `last_use[alloc_id]`
    // over all allocation sites that resolved to r. Under unique-per-alloc each
    // region has exactly one contributing alloc_id; the max is kept so the
    // result stays correct should any region ever gather more than one.
    let mut du = DefUseBuilder::new();
    du.walk(hir);

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
        &top_level_reassigns,
        &local_reassigns,
        &escape_info,
    );

    // Explicit structural execution-order index. `decref_point` selection
    // compares these indices, never `HirId` magnitude (which ANF
    // makes meaningless — see `compute_order`).
    let order = compute_order(hir);
    let last_use_info = compute_last_use(hir, &du.uses, &order);

    // Populate and extend every region's `decref_point`: alloc/cell seeds,
    // binding-chain extension, env-cell loop hoist, and the return/destructure
    // consuming-node pins (see `decref`).
    decref::populate_decref_points(
        &mut info,
        hir,
        &du,
        &order,
        &last_use_info,
        &inference_binding_regions,
        &return_sites,
        &destructure_sites,
    );
    let last_use = &last_use_info.per_node;

    // Per-path branch compensation (`regions::compensate`): a region whose single
    // `decref_point` sits inside a conditional arm is freed there on the used
    // path but leaks on the sibling arms; add a compensating release at each dead
    // sibling arm's head. Reads the FINAL `region_data` (the decref_point post-passes
    // above) and the exclusion sets, so it runs after them. Independent of the merge
    // seed below (a merge child is excluded), but placed before it for locality.
    let branch_comp = super::compensate::compute_branch_compensation(
        hir,
        &info,
        &escape_info,
        &du,
        arena,
        &order,
        last_use,
    );
    info.branch_compensation = branch_comp.head;
    info.branch_arm_decrefs = branch_comp.tail;
    info.container_release_sites = branch_comp.container_release_sites.into_iter().collect();

    // The builder-idiom merge seed (docs/impl/region/merging.md § Merging). Runs
    // LAST: its coincident-decref_point gate reads the final `region_data`, so it
    // must follow every decref_point post-pass above. The lowerer consumes the
    // resulting `merged_parent` forest through `static_slot`'s `merged_root`
    // canonicalization (one slot per merge tree); an empty forest leaves it the
    // identity, i.e. the unmerged baseline.
    info.merged_parent = super::merge::compute_merges(hir, arena, &info, &escape_info, &order);

    // The letrec closure-cycle merge (docs/impl/region/letrec.md § The letrec
    // closure-cycle merge): a self/mutual
    // recursive closure SCC and its prebound capture cells collapse onto one arena,
    // extending the same `merged_parent` forest and riding the same `merged_root`
    // canonicalization as the builder-idiom seed. Unconditional (not flag-gated), so it
    // lands on every tier. The single `DecrefRegion` fires at the root region's
    // `decref_point`, which is set here to the cycle's binding scope — the `letrec`
    // that prebinds the members' capture cells (region/adopt.md § The lifetime
    // obligation the root carries). Its scope-exit post-dominates every direct use of
    // the members (they are bound there), while a foreign capture of a member is
    // RC-counted and outlives the single decref. Runs after the builder seed (it
    // shares the map) and before the ownership pass (so `is_merged` excludes these
    // members).
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
        // strands the binding-scope drop past the frame-replacing `TailCall`; the
        // lowerer keys `deferred_release_slot = static_slot(root)` at each such site so a
        // closure callee's frame replacement is balanced by the activation-completion
        // adopt (region/letrec.md § The letrec closure-cycle merge). A member tail
        // keeps its own `stranded_cycle_bindings` channel and is not recorded here.
        for &site in &cm.tail_release_sites {
            info.cycle_tail_release.insert(site, cm.root);
        }
        info.region_data
            .entry(cm.root)
            .and_modify(|d| d.decref_point = cm.drop_site)
            .or_insert(RegionData {
                decref_point: cm.drop_site,
            });
    }

    // Ownership forest: adopt edges, the transferred-returned-subtree cut, the
    // co-owned-cycle cut, and the activation-owner cut. Runs LAST, after the
    // final `region_data` and the merge seeds above (see `adopt`).
    adopt::apply_ownership(
        &mut info,
        hir,
        &escape_info,
        arena,
        &order,
        &transfer_call_class,
    );

    info
}
