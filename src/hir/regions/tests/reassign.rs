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
/// the cell's reference — balanced.
#[test]
fn reassign_gate_keeps_container_stored_value_top_level() {
    let (hir, _, info) = pipeline(
        "(var keeper (%pair nil nil))\n\
         (var x (%pair 1 2))\n\
         (%array-push keeper x)\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    // Precondition anchoring the boundary: the push records a
    // cross-region edge at a site that is NOT one of x's assign sites.
    let assign_ids: Vec<HirId> = sites.iter().map(|(id, _)| *id).collect();
    assert!(
        info.cross_region_refs
            .iter()
            .any(|(site, _, _)| !assign_ids.contains(site)),
        "precondition: the push must record an edge at a non-assign site"
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
         (h 1 (%pair nil nil))",
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

/// Counterfactual to the donation split: a FN-LOCAL 1-slot container KEEPS its
/// assign-value decref (its scope-exit demise), so its producer reference is
/// released on its own and the cell must take a counted incref-on-store — which
/// drop-on-overwrite balances. So a fn-local reassign is a drop-on-overwrite site
/// but must NOT be donated; donating it (skipping the incref-on-store) would free
/// the value at its producer decref before drop-on-overwrite loads it (a UAF). The
/// conditional keeps the Assign alive through functionalization (a straight-line
/// fn-local reassign is rewritten to a shadowing let and never reaches the gate).
#[test]
fn donated_overwrite_excludes_fn_local_reassign() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (%pair 1 2))\n\
                  (%array-push (%pair nil nil) x)\n\
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
    for (site, _) in &sites {
        assert!(
            !info.donated_overwrite_sites.contains(site),
            "a fn-local reassign must NOT be donated — it keeps the assign-value \
             decref and takes a counted incref-on-store (site @{})",
            site.0
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
         (%first acc)",
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
