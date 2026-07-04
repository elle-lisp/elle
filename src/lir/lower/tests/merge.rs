use super::*;

// ── Transform 1 at lower_return: the coalesced return mint ───────────────────
//
// `lower_return` selects the return mint's encoding by the staticness predicate:
// a coalescible return — a fresh local allocation whose region is a known slot —
// lowers its mint to the slot-resolved `IncrefRegion` (guarded under
// `debug_assertions` by the equivalence oracle `AssertRegionMatches` on the same
// slot), instead of the value-resolved `IncrefValueRegion`. A refused return (the
// dynamic boundary — a parameter, an immediate, a pass-through) keeps
// `IncrefValueRegion`. Spec: docs/impl/region-rules.md § "Compile-time region
// selection (coalescing)".
//
// These pins are counterfactual against a value-resolved-at-every-tail lowerer:
// the fresh-allocation pins (RED before the substitution) assert it happened; the
// param pin guards the refusal half against over-coalescing (a slot resolving to
// the wrong physical region is a UAF).

#[test]
fn coalesced_fresh_pair_return_is_slot_resolved() {
    // `(fn () (%pair 1 2))` — the inner closure tail-returns a fresh `%pair` (a
    // `List` allocation in its own live region). Its return mint must coalesce to
    // a slot-resolved `IncrefRegion`. Counterfactual: value-resolved, the closure
    // is `[List, IncrefValueRegion]` (one IncrefValueRegion, zero IncrefRegion) — RED.
    let module = compile_to_lir("(fn () (%pair 1 2))");
    let pair_fn = module
        .closures
        .iter()
        .find(|f| func_count(f, |i| matches!(i, LirInstr::List { .. })) == 1)
        .expect("a closure tail-allocating a %pair (List)");
    assert_eq!(
        func_count(pair_fn, |i| matches!(i, LirInstr::IncrefValueRegion { .. })),
        0,
        "the fresh-pair return mint must NOT stay value-resolved",
    );
    assert_eq!(
        func_count(pair_fn, |i| matches!(i, LirInstr::IncrefRegion { .. })),
        1,
        "the fresh-pair return mint must be slot-resolved (one IncrefRegion)",
    );
    assert_coalesced_oracle_precedes(pair_fn);
}

#[test]
fn coalesced_string_literal_return_is_slot_resolved() {
    // `(fn () "hi")` — a returned string literal is a `MaterializeConst`
    // allocation in its own live region (`coalescible_accepts_returned_string_literal`),
    // so its return mint coalesces. Value-resolved, the closure is
    // `[MaterializeConst, IncrefValueRegion]` — RED.
    let module = compile_to_lir("(fn () \"hi\")");
    let str_fn = module
        .closures
        .iter()
        .find(|f| func_count(f, |i| matches!(i, LirInstr::MaterializeConst { .. })) == 1)
        .expect("a closure tail-allocating a string literal (MaterializeConst)");
    assert_eq!(
        func_count(str_fn, |i| matches!(i, LirInstr::IncrefValueRegion { .. })),
        0,
        "the string-literal return mint must NOT stay value-resolved",
    );
    assert_eq!(
        func_count(str_fn, |i| matches!(i, LirInstr::IncrefRegion { .. })),
        1,
        "the string-literal return mint must be slot-resolved (one IncrefRegion)",
    );
    assert_coalesced_oracle_precedes(str_fn);
}

// ── Transform 1 at the two narrow reassign sites ─────────────────────────────
//
// The two remaining transform-1 sites are both reassigned-binding traffic over a
// value the lowerer just allocated locally:
//   - the reassign incref-on-store (`lower_assign`'s drop-on-overwrite): a fn-local
//     1-slot container's fresh new content coalesces its pin to `IncrefRegion`;
//   - the captured-reassign init-drop (`store_captured_cell_init`): the producer's
//     reference to a captured binding's fresh init value coalesces its drop to a
//     slot-resolved `DecrefRegion` (the decref side — `DecrefValueRegion` →
//     `DecrefRegion`).
// Both gate on `coalescible_region` and guard the slot-resolved instruction under
// `debug_assertions` with `AssertRegionMatches`. A module-scope container's value
// stays value-resolved (it is in `mutated_binding_value_regions`, the runtime fact
// the container tracks), as does the drop-old of the displaced content. Spec:
// docs/impl/region-rules.md § "Compile-time region selection (coalescing)".

#[test]
fn captured_reassign_init_drop_is_slot_resolved() {
    // `@x` is captured by `u` (forcing needs_capture → a `MakeCaptureCell` init)
    // AND reassigned at module scope, so it lands in `captured_reassigns`:
    // `store_captured_cell_init` runs with reassigned=true and drops the producer's
    // reference to the fresh init `(%pair 1 2)`. That init is a fresh local
    // allocation whose region is a known slot, so the drop coalesces to a
    // slot-resolved `DecrefRegion` (the decref side of transform 1), guarded under
    // debug by `AssertRegionMatches` on the same slot. Counterfactual: value-resolved,
    // the init's StoreCaptureCell is immediately followed by a value-resolved
    // `DecrefValueRegion` — this test then panics.
    let module = compile_to_lir(
        "(begin (def @x (%pair 1 2)) (def u (fn () x)) (assign x (%pair 3 4)) (g (u)))",
    );
    let instrs = flat_instrs(&module.entry);
    let mut found = false;
    for (i, instr) in instrs.iter().enumerate() {
        // The init-drop is the region-RC release emitted immediately after the
        // init's StoreCaptureCell (the reassignment's StoreCaptureCell is followed
        // by a LoadLocal, never a release).
        if !matches!(instr, LirInstr::StoreCaptureCell { .. }) {
            continue;
        }
        let (drop_pos, oracle) = match instrs.get(i + 1) {
            Some(LirInstr::AssertRegionMatches { region_id, .. }) => (i + 2, Some(*region_id)),
            _ => (i + 1, None),
        };
        match instrs.get(drop_pos) {
            Some(LirInstr::DecrefRegion { region_id }) => {
                found = true;
                if cfg!(debug_assertions) {
                    assert_eq!(
                        oracle,
                        Some(*region_id),
                        "debug builds must emit AssertRegionMatches on the SAME slot \
                         immediately before the coalesced init-drop DecrefRegion",
                    );
                } else {
                    assert!(oracle.is_none(), "release builds emit no oracle");
                }
            }
            Some(LirInstr::DecrefValueRegion { .. }) => panic!(
                "the captured-reassign init-drop is still value-resolved \
                 (DecrefValueRegion after StoreCaptureCell) — transform 1's decref \
                 side did not coalesce",
            ),
            _ => {}
        }
    }
    assert!(
        found,
        "expected a StoreCaptureCell followed by the coalesced init-drop DecrefRegion \
         in (begin (def @x (%pair 1 2)) (assign x …) …)",
    );
}

#[test]
fn param_return_stays_value_resolved() {
    // The dynamic boundary: a return whose value is NOT a fresh local allocation
    // — a parameter, nil, a rest-arg — has no statically nameable region and must
    // keep the value-resolved `IncrefValueRegion`. In `(fn (x) x)` neither the
    // user closure (returns its param) nor the letrec stub closures (return
    // nil/rest-args) allocate, so NO non-allocating closure may slot-resolve its
    // return mint — that would be an over-coalesce (a UAF in waiting). This guards
    // the refusal half of the predicate at the emission site (stable across C2).
    let module = compile_to_lir("(fn (x) x)");
    let mut saw_passthrough = false;
    for f in &module.closures {
        if allocates_or_calls(f) {
            continue; // an allocating/calling closure may coalesce its own tail
        }
        if func_count(f, |i| matches!(i, LirInstr::IncrefValueRegion { .. })) > 0 {
            saw_passthrough = true;
        }
        assert_eq!(
            func_count(f, |i| matches!(i, LirInstr::IncrefRegion { .. })),
            0,
            "a closure that allocates nothing must not slot-resolve its return \
             mint (over-coalesce of a param/immediate — the dynamic boundary)",
        );
    }
    assert!(
        saw_passthrough,
        "expected at least one pure passthrough closure (the `(fn (x) x)` param return)",
    );
}

// ── The builder-idiom merge flip ─────────────────────────────────────────────
//
// The merge flip (docs/impl/region-model.md § "Emission: one slot per merge tree,
// one demise at the root"): a builder-idiom merge collapses a fresh child
// aggregate into the parent `%pair` it is stored into, so child and parent
//   (1) allocate against ONE static slot — the root's; `static_slot` canonicalizes
//       every region through `merged_root`;
//   (2) carry ONE `DecrefRegion` — the root's; the non-root child's own demise is
//       suppressed (only the root's drop frees the shared region);
//   (3) drop the now-intra-region `child→parent` store edge's `IncrefRegion`
//       (transform 2 — the cascade skips self-references, so keeping it leaks);
//   (4) record the shared slot in `merged_slots` for runtime mint-or-reuse.
//
// The canonical shape is the discarded nested literal `(begin (%pair (%pair 1 2) 3)
// nil)` — exactly the source the merge-seed pins in `hir/regions/tests.rs` fire on,
// under the unit harness's `--checked-intrinsics=off` default (where `%pair` is a
// `Pair` intrinsic, lowered to `LirInstr::List`, so the seed has sites to merge).
//
// Each pin is counterfactual against the pre-flip emission, which allocates child
// and parent on DISTINCT slots, emits TWO `DecrefRegion`s, KEEPS the self-edge
// `IncrefRegion`, and records NO `merged_slots`. Written from the spec, not from
// emission output (CLAUDE.md).

#[test]
fn merge_flip_child_and_parent_share_one_slot() {
    // (1) Both `%pair` allocations resolve to ONE static slot — the merged root's.
    // Counterfactual: pre-flip `static_slot` mints a distinct slot per region, so
    // the child and parent pair carry two distinct slots → the distinct count is 2.
    let module = compile_to_lir(BUILDER_IDIOM);
    let slots = builder_pair_slots(&module);
    assert_eq!(
        slots.len(),
        2,
        "the nested literal must lower to two %pair (List) allocs; got {slots:?}",
    );
    let distinct: std::collections::HashSet<StaticRegion> = slots.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        1,
        "the merged child and parent pair must allocate against ONE static slot \
         (the root's — static_slot canonicalizes through merged_root); got slots {slots:?}",
    );
}

#[test]
fn merge_flip_emits_one_decref_for_the_merged_pair() {
    // (2) The merged region has exactly ONE `DecrefRegion` (the root's), naming the
    // shared slot. Counterfactual: pre-flip child and parent each carry their own
    // `DecrefRegion` on distinct slots → two decrefs over the pair-slot set.
    let module = compile_to_lir(BUILDER_IDIOM);
    let slots: std::collections::HashSet<StaticRegion> =
        builder_pair_slots(&module).into_iter().collect();
    let decrefs = func_count(
        &module.entry,
        |i| matches!(i, LirInstr::DecrefRegion { region_id } if slots.contains(region_id)),
    );
    assert_eq!(
        decrefs, 1,
        "the merged builder pair must carry exactly ONE DecrefRegion (the root's; the \
         non-root child's own demise is suppressed); got {decrefs} over slots {slots:?}",
    );
}

#[test]
fn merge_flip_drops_the_self_edge_incref() {
    // (3) The `child→parent` store edge is intra-region post-merge, so its
    // `IncrefRegion` is dropped (transform 2). Counterfactual: pre-flip the edge
    // emits one `IncrefRegion` on the child slot → one incref over the pair-slot set.
    let module = compile_to_lir(BUILDER_IDIOM);
    let slots: std::collections::HashSet<StaticRegion> =
        builder_pair_slots(&module).into_iter().collect();
    let increfs = func_count(
        &module.entry,
        |i| matches!(i, LirInstr::IncrefRegion { region_id } if slots.contains(region_id)),
    );
    assert_eq!(
        increfs, 0,
        "the merged child→parent self-edge IncrefRegion must be dropped (the free-time \
         cascade skips self-references, so keeping it leaks); got {increfs} over slots {slots:?}",
    );
}

#[test]
fn merge_flip_records_merged_slot_metadata() {
    // (4) The shared slot is recorded in the function's `merged_slots` so the
    // runtime mint-or-reuses it (child mints, parent reuses → one physical region).
    // Counterfactual: pre-flip `record_merged_slots` finds the child and parent on
    // distinct slots, records nothing → `merged_slots` is empty.
    let module = compile_to_lir(BUILDER_IDIOM);
    let pair_slots: std::collections::HashSet<StaticRegion> =
        builder_pair_slots(&module).into_iter().collect();
    assert!(
        !module.entry.merged_slots.is_empty(),
        "the builder-idiom merge must record a merged slot for runtime mint-or-reuse; \
         got empty merged_slots",
    );
    assert!(
        module
            .entry
            .merged_slots
            .iter()
            .any(|s| pair_slots.contains(s)),
        "a recorded merged slot must be one the builder pairs allocate against; \
         merged_slots={:?}, pair_slots={:?}",
        module.entry.merged_slots,
        pair_slots,
    );
}

#[test]
fn merge_flip_inert_without_a_merge() {
    // The flip is gated on a merge actually firing: a single fresh `%pair` (no
    // nested child to merge) records no merged slot, drops no edge, and keeps its
    // one ordinary `DecrefRegion` — the one-region-per-value baseline. This pins
    // that the canonicalization/suppression/drop touch nothing when `merged_parent`
    // is empty (the same inertness the `--checked-intrinsics=on` default relies on,
    // where no `%pair` survives as an intrinsic at all).
    let module = compile_to_lir("(begin (%pair 1 2) nil)");
    let slots = builder_pair_slots(&module);
    assert_eq!(
        slots.len(),
        1,
        "a single %pair has one List alloc; got {slots:?}"
    );
    assert!(
        module.entry.merged_slots.is_empty(),
        "a lone %pair (no child to merge) must record no merged slot; got {:?}",
        module.entry.merged_slots,
    );
    let slot = slots[0];
    let decrefs = func_count(
        &module.entry,
        |i| matches!(i, LirInstr::DecrefRegion { region_id } if *region_id == slot),
    );
    assert_eq!(
        decrefs, 1,
        "the lone discarded %pair keeps its one ordinary DecrefRegion"
    );
}

// ── C7: the RC-coalescing measurement instrument ─────────────────────────────
//
// `rcstats` (src/lir/lower/rcstats.rs) records, per coalescing-candidate site,
// whether the mint resolved to a static slot or stayed value-resolved, plus the
// transform-2 self-edges eliminated — the data `benches/regionrc.rs` reports as
// "the measured win" (verona Stage 5 § Tests-first). The decision is NOT
// recoverable from the final LIR (a coalesced mint's `IncrefRegion` is
// indistinguishable from a store-edge's, and an eliminated edge leaves no
// instruction), so the lowerer records it at the decision site; these pins prove
// it observes the same decisions the C2/C3/C6 emission pins above assert.
//
// Counterfactual: before the `rcstats::record_*` calls were wired into the
// lowerer the counters stayed zero on every compile, so each `>= 1` assertion
// below was RED. `reset`+`snapshot` bracket one compile on this thread.

#[test]
fn rcstats_counts_coalesced_return_mint() {
    // `(fn () (%pair 1 2))` slot-resolves its fresh-pair return mint
    // (`coalesced_fresh_pair_return_is_slot_resolved` pins the emitted
    // `IncrefRegion`); the instrument must count it as a slot-resolved return.
    rcstats::reset();
    let _ = compile_to_lir("(fn () (%pair 1 2))");
    let s = rcstats::snapshot();
    assert!(
        s.return_mint_slot >= 1,
        "a fresh-pair return must record >= 1 slot-resolved return mint; got {s:?}",
    );
}

#[test]
fn rcstats_counts_value_resolved_return_mint() {
    // `(fn (x) x)` returns its parameter — the dynamic boundary
    // (`param_return_stays_value_resolved`); the instrument must count it as a
    // value-resolved return, never a slot.
    rcstats::reset();
    let _ = compile_to_lir("(fn (x) x)");
    let s = rcstats::snapshot();
    assert!(
        s.return_mint_value >= 1,
        "a param return must record >= 1 value-resolved return mint; got {s:?}",
    );
}

#[test]
fn rcstats_counts_self_edge_eliminated() {
    // The builder idiom `(begin (%pair (%pair 1 2) 3) nil)` merges the inner pair
    // into the outer and drops the resulting intra-region self-edge
    // (`merge_flip_drops_the_self_edge_incref`); the instrument must count the
    // elimination.
    rcstats::reset();
    let _ = compile_to_lir(BUILDER_IDIOM);
    let s = rcstats::snapshot();
    assert!(
        s.self_edges_eliminated >= 1,
        "the builder idiom must record >= 1 eliminated self-edge; got {s:?}",
    );
}

#[test]
fn rcstats_slot_fraction_tracks_coalesced_over_candidates() {
    // The derived `slot_fraction` is coalesced / (coalesced + value-resolved)
    // over the three transform-1 sites — `None` only when there were no candidate
    // mints at all. A compile with at least one return mint always has candidates.
    rcstats::reset();
    let _ = compile_to_lir("(fn () (%pair 1 2))");
    let s = rcstats::snapshot();
    let frac = s
        .slot_fraction()
        .expect("a compile with return mints has candidates");
    assert!(
        (0.0..=1.0).contains(&frac),
        "slot_fraction must be in [0,1]; got {frac} from {s:?}",
    );
    assert_eq!(
        s.coalesced() + s.value_resolved(),
        s.return_mint_slot
            + s.return_mint_value
            + s.reassign_store_slot
            + s.reassign_store_value
            + s.captured_init_slot
            + s.captured_init_value,
        "coalesced + value_resolved must total every transform-1 candidate site",
    );
}
