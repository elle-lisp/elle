use super::*;

// ── 1-slot-container gate: sole-held AND not-returned ───────────────
//
// docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot containers":
// the drop-on-overwrite + suppression model may be applied only when
// every region the cell may hold is sole-held by the binding AND not
// claimed by ownership transfer at a return/tail boundary (two static
// owners of one initial reference is a double-free). Runtime-counted
// escapes — container stores, captures, opaque-call cliques — are
// value-based-balanced and MUST keep the gate: refusing them regresses
// region-mutable-reassign-{selfref,branch,flow} and
// region-toplevel-{mutable-reassign,reassign-thunk-uaf} straight into
// UAFs. The `keeps` tests below are the counterfactual pins against
// that over-exclusion.

/// Boundary pin (counterfactual against over-exclusion): a TOP-LEVEL
/// reassigned mutable whose prior value is stored into another
/// container before the overwrite KEEPS the container model. The store
/// is runtime-counted (the push funnel increfs; the keeper's free
/// cascade decrefs), so the cell's drop-on-overwrite releases exactly
/// the cell's reference — balanced. Analyzed under the real
/// classification so the push resolves to its `Funnel` effect.
#[test]
fn reassign_gate_keeps_container_stored_value_top_level() {
    let (hir, info) = analyze_full(
        "(var keeper @[])\n\
         (var x (%pair 1 2))\n\
         (%array-push keeper x)\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    // Precondition anchoring the boundary: the funnel push records its stored
    // value at a site that is NOT one of x's assign sites.
    let assign_ids: Vec<HirId> = sites.iter().map(|(id, _)| *id).collect();
    assert!(
        info.funnel_store_sites
            .keys()
            .any(|site| !assign_ids.contains(site)),
        "precondition: the push must record a funnel store at a non-assign site"
    );
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "a runtime-counted container store must not refuse the gate"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "a runtime-counted container store must keep the suppression"
    );
}

/// Boundary pin (counterfactual against over-exclusion): the FN-LOCAL
/// variant — the init value stored into a parameter container before a
/// conditional overwrite keeps the model. (The conditional keeps the
/// Assign alive through functionalization; a straight-line fn-local
/// reassign is rewritten into a shadowing let and never reaches the
/// gate.)
#[test]
fn reassign_gate_keeps_container_stored_value_fn_local() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c keeper)\n\
           (begin (var x (%pair 1 2))\n\
                  (%array-push keeper x)\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  nil)))\n\
         (h 1 @[])",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "a runtime-counted fn-local container store must not refuse the gate"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "a runtime-counted fn-local container store must keep the suppression"
    );
}

/// A reassigned binding whose value is RETURNED must not get the
/// container model: the return transfers the value's single initial
/// reference to the caller while the model claims it for the cell —
/// two static owners of one reference. Today this shape is refused
/// twice over (the read-after-assign is phi-wrapped, so the phi
/// binding alias fails sole_held; and the tail regions land in
/// `returned_regions`); the pin holds either derivation accountable so
/// neither can be silently voided.
#[test]
fn reassign_gate_refuses_returned_value() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (%pair 1 2))\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  x)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    for (site, _) in &sites {
        assert!(
            !info.drop_on_overwrite_sites.contains(site),
            "returned value: the gate must refuse drop-on-overwrite at @{}",
            site.0
        );
    }
    assert!(
        info.suppressed_decref_regions.is_empty(),
        "returned value: no decref may be suppressed (got {:?})",
        info.suppressed_decref_regions
    );
}

/// Stay-GREEN control: the plain sole-held, not-returned top-level
/// reassign keeps the container model — drop-on-overwrite at the assign
/// and suppression of the cell's value regions. Guards the exclusion
/// against over-refusal: the cell-store edge recorded AT the assign
/// site itself must not count against the gate.
#[test]
fn reassign_gate_applies_to_sole_held_unescaped() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "sole-held unescaped reassign must keep drop-on-overwrite"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "sole-held unescaped reassign must suppress the cell's value-region decrefs"
    );
}

/// The mutated-slot backstop (docs/impl/region/bindings.md "The fallback's
/// value route is not unconditionally safe"). A top-level reassigned binding's
/// init + assign-value regions are recorded in `mutated_binding_value_regions`
/// UNCONDITIONALLY — even when the suppression gate succeeds — so the lowerer
/// never value-routes their release through the mutated cell slot. This is the
/// safety net for the no-alias-corruption UAF (region-mutable-reassign-flow
/// facet 3), where a `(deref-cell x)` read is solved to the cell's init region
/// and would otherwise route the init's decref through the reassigned slot.
#[test]
fn mutated_slot_backstop_records_top_level_reassign_value_regions() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    // Every region the cell's static decrefs are suppressed for must ALSO be in
    // the backstop set: the backstop is the lowerer-side guarantee that even if
    // the suppression were absent (the gate-fail fallback), the value route is
    // never emitted through the mutated slot.
    assert!(
        !info.mutated_binding_value_regions.is_empty(),
        "top-level reassign must record its value regions in the backstop set"
    );
    assert!(
        info
            .suppressed_decref_regions
            .iter()
            .all(|r| info.mutated_binding_value_regions.contains(r)),
        "every suppressed region must also be in the backstop set (got backstop={:?}, suppressed={:?})",
        info.mutated_binding_value_regions,
        info.suppressed_decref_regions,
    );
}

/// The producer-reference donation split (docs/impl/region/bindings.md
/// "Reassigned mutable bindings are 1-slot containers"). A
/// MODULE-SCOPE (file-letrec) 1-slot container suppresses its assign-value
/// region's ordinary decref, donating the producer's reference to the cell — so
/// the lowerer must NOT incref-on-store, and the site is recorded in
/// `donated_overwrite_sites`. Without that marker an unbalanced incref-on-store
/// holds every displaced prior to teardown (the reassign-in-loop over-keep).
/// Every donated site is also a drop-on-overwrite site (donation is a refinement
/// of the container model, not a separate path).
#[test]
fn donated_overwrite_marks_module_scope_reassign() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.donated_overwrite_sites.contains(site)),
        "a module-scope 1-slot container's overwrite must be marked donated \
         (cell adopts the producer reference — no incref-on-store)"
    );
    assert!(
        info.donated_overwrite_sites
            .iter()
            .all(|s| info.drop_on_overwrite_sites.contains(s)),
        "every donated site must also be a drop-on-overwrite site (got donated={:?}, \
         drop={:?})",
        info.donated_overwrite_sites,
        info.drop_on_overwrite_sites,
    );
}

/// The fn-local donation split runs on the CONTENT's producer discipline:
///
/// - STATIC-ALLOC content (`(assign x (%pair 3 4))` — a slot-resolved fresh
///   mint) is DONATED: its birth reference becomes the cell's reference,
///   released by drop-on-overwrite (each displaced prior at its overwrite)
///   and by the kept scope-exit slot `DecrefRegion` (the final mint). An
///   incref-on-store would strand every displaced prior at rc 1 — one
///   region per loop iteration (`(assign acc (%pair i i))` in a loop leaks
///   its whole history without the donation).
/// - CALL-RESULT content (`(assign x (array 3 4))`) keeps the counted
///   incref-on-store: its producer claim is released by its own
///   value-resolved `DecrefValueRegion`, so donating (skipping the store
///   incref) would free the value before drop-on-overwrite loads it (UAF).
///
/// The conditional keeps the Assign alive through functionalization (a
/// straight-line fn-local reassign is rewritten to a shadowing let and never
/// reaches the gate).
#[test]
fn donated_overwrite_splits_fn_local_reassign_by_content_producer() {
    // Static-alloc content: donated.
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (%pair 1 2))\n\
                  (%array-push @[] x)\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  nil)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "precondition: the fn-local container store keeps the gate (drop-on-overwrite)"
    );
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.donated_overwrite_sites.contains(site)),
        "static-alloc content donates its birth reference to the fn-local cell"
    );

    // Call-result content: counted store, never donated.
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (array 1 2))\n\
                  (%array-push @[] x)\n\
                  (if c (assign x (array 3 4)) nil)\n\
                  nil)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    for (site, _) in &sites {
        assert!(
            !info.donated_overwrite_sites.contains(site),
            "call-result content keeps the counted incref-on-store — donating \
             would free it at its own producer decref before drop-on-overwrite \
             (site @{})",
            site.0
        );
    }
}

/// Facet A of the captured-mutable read mis-coalesce
/// (integration::file_scope::captures::test_mutable_var_mutation_visible_after_call):
/// a `(begin (var x …) …)` single-form file's `x` is a compiled Begin-pre-pass
/// CaptureCell that is RE-STORED from inside a sibling closure — neither
/// module-scope-classified (`captured_reassigns` needs `is_file_scope`) nor
/// fn-local. A whole-value read through the cell is solved to the CELL's own
/// region, which must be poisoned in `mutated_binding_value_regions` so
/// `coalescible_region` refuses the static route (the return retain stays
/// value-resolved instead of resolving the cell's slot against repointed
/// content — the `AssertRegionMatches` mis-coalesce).
#[test]
fn mutated_slot_backstop_poisons_restorable_begin_cell_regions() {
    let (hir, arena, info) = pipeline(
        "(begin\n\
           (var x (%pair 1 2))\n\
           (def bump (fn () (assign x (%pair 3 4))))\n\
           (bump)\n\
           x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    let b = sites[0].1;
    assert!(
        arena.get(b).is_restorable_capture_cell(),
        "precondition: x must be a re-storable capture cell"
    );
    let cell_regions: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flatten()
        .filter(|(bb, _)| *bb == b)
        .map(|(_, r)| *r)
        .collect();
    assert!(
        !cell_regions.is_empty(),
        "precondition: x must have a compiled Begin-pre-pass cell region"
    );
    for r in cell_regions {
        assert!(
            info.mutated_binding_value_regions.contains(&r),
            "re-storable compiled cell region {:?} must be in the mutated-slot backstop",
            r
        );
    }
}

/// Facet B: the multi-form file, where the trailing `x` read lifts into a
/// file-letrec statement wrapper `[__file_expr_N (deref-cell x)]`. The Letrec
/// arm must apply the same Rule 5 counted-reader treatment as the Let arm
/// (`counted_cell_read_regions`): the wrapper's source region must be a fresh
/// call-result placeholder (value-resolved, counted at the read), NOT the
/// init value's own region — a static route against the init region frees or
/// retains the wrong region once the cell is repointed.
#[test]
fn letrec_wrapper_read_of_restorable_cell_is_counted() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (def bump (fn () (assign x (%pair 3 4))))\n\
         (bump)\n\
         x",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "the file-letrec wrapper's whole-value read of the re-storable cell \
         must be a counted read (Rule 5 reader retain)"
    );
    // The init %pair's region must NOT be reachable as any OTHER binding's
    // single source region (the wrapper must not inherit it): every
    // counted-read site's placeholder is a call-result region, refused by
    // `coalescible_solver_region`.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {:?} must be a call-result region",
            r
        );
    }
}

/// Stay-GREEN control: the self-referential accumulator
/// `(assign acc (%pair i acc))` stays gated. Its only cross-region
/// edges are the model's own (the cell-store edge at the assign site;
/// value-succession collapses to a self-edge in a loop's single static
/// region), so the exclusion must not refuse it — refusing would
/// reintroduce the unsuppressed baseline's per-iteration over-keep for
/// the canonical accumulation idiom (region-mutable-reassign-selfref).
#[test]
fn reassign_gate_keeps_selfref_accumulator() {
    let (hir, _, info) = pipeline(
        "(var acc ())\n\
         (var i 0)\n\
         (while (%lt i 3) (begin (assign acc (%pair i acc)) (assign i (%add i 1))))\n\
         (length acc)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain reassigns");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "self-referential accumulator must keep drop-on-overwrite"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "self-referential accumulator must keep its suppression"
    );
}
