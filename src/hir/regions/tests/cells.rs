use super::*;

// ── Begin/Letrec alloc_region only when MakeCaptureCell will be emitted ──
//
// The Begin and Letrec walkers register an alloc_region so the lowerer's
// `emit_alloc(MakeCaptureCell)` can find a region. But the lowerer's
// `lower_begin`/`lower_letrec` only emit MakeCaptureCell when (a) there
// is at least one reachable binding with `needs_capture()` true and (b)
// the current lowering is NOT inside a lambda body (inside a lambda the
// VM materializes cells via the closure-construction path). When neither
// condition holds, no emit_alloc fires — and an unconditional alloc_here
// produces a phantom region whose later DecrefRegion targets a region
// the runtime never created. The lowerer's `emit_decref_region` guard
// silently swallows the phantom in release builds, but the runtime
// debug_assert in `regionstore.decref` catches it in debug.
//
// Surfaced by hir::functionalize::tests::if_phi_merge_with_continuation_still_works:
// the synthetic phi-merge let inserted by functionalize introduces a Begin
// wrapper, and even the outer Letrec wrapping the test's top-level form
// has no needs_capture binding when the test's `(var x 0)` isn't captured
// by any lambda — so all three of (Letrec, outer Begin, inner Begin)
// produce phantom regions.

#[test]
fn begin_with_no_captured_binding_has_no_alloc_region() {
    // `(do (var x 0) (if true (assign x 42) (assign x 99)) x)` — the
    // var x is never captured by a closure, so `needs_capture()` is
    // false for every binding reachable from any Begin in this tree.
    // The lowerer will not emit MakeCaptureCell for any Begin here;
    // the regions walker must not assign an alloc_region to any Begin.
    let (hir, _arena, info) = pipeline("(do (var x 0) (if true (assign x 42) (assign x 99)) x)");
    let begins = find_begins(&hir);
    assert!(
        !begins.is_empty(),
        "expected at least one Begin in the functionalized HIR"
    );
    for begin_id in &begins {
        assert!(
            !info.alloc_region.contains_key(begin_id),
            "Begin @{} should not have an alloc_region — no reachable binding needs_capture",
            begin_id.0
        );
    }
}

#[test]
fn letrec_with_no_captured_binding_has_no_alloc_region() {
    // Same shape, but checking the Letrec — file-scope wrappers like
    // the synthetic `(letrec [__file_expr_0 ...] __file_expr_0)` that
    // `compile_file_to_fhir` inserts have no captured binding either.
    // (Lowerer only emits MakeCaptureCell when a letrec binding's
    // `needs_capture()` is true and we're not inside a lambda.)
    let (hir, arena, info) = pipeline("(do (var x 0) (if true (assign x 42) (assign x 99)) x)");
    fn check(h: &Hir, arena: &BindingArena, info: &RegionInfo) {
        if let HirKind::Letrec { bindings, .. } = &h.kind {
            let any_captured = bindings.iter().any(|(b, _)| arena.get(*b).needs_capture());
            if !any_captured {
                assert!(
                    !info.alloc_region.contains_key(&h.id),
                    "Letrec @{} should not have an alloc_region — no binding needs_capture",
                    h.id.0
                );
            }
        }
        h.for_each_child(|c| check(c, arena, info));
    }
    check(&hir, &arena, &info);
}

#[test]
fn begin_with_captured_define_has_alloc_region() {
    // Positive: when a Begin contains a Define for a binding that IS
    // captured by a nested lambda, the lowerer DOES emit MakeCaptureCell;
    // the regions walker must register the Begin in alloc_region.
    // Using `def @x` (mutable) + `(fn () x)` (captures x) makes
    // `needs_capture()` true for x.
    let (hir, _arena, info) = pipeline("(do (var x 0) (def f (fn () x)) (assign x 1) (f))");
    let begins = find_begins(&hir);
    assert!(!begins.is_empty(), "expected at least one Begin");
    // Per-cell regions live in `begin_cell_regions` (one region PER captured
    // binding — a single shared alloc_region for all of a Begin's cells is
    // the shared-slot capture-cell leak; see docs/impl/region-model.md, "one
    // allocation execution per slot between drops").
    let any_cells = begins.iter().any(|id| {
        info.begin_cell_regions
            .get(id)
            .is_some_and(|v| !v.is_empty())
    });
    assert!(
        any_cells,
        "at least one Begin must register begin_cell_regions — a captured Define is present"
    );
}

/// Counter-factual for the destructure-binding-regions overwrite bug.
///
/// At top level, `(begin (def (a & r) (list 1 2 3)) (length r))` lowers
/// to a letrec whose first init is the begin (which side-effects
/// `binding_regions[r] = [list_region]` via the destructure) and whose
/// subsequent inits include `r = nil` as the placeholder for the
/// pattern binding. The letrec walk must UNION per-init contributions
/// rather than overwrite: the `r = nil` init contributes no regions, so
/// a blind insert would replace the destructure's `binding_regions[r]`
/// with `[]`. That would leave the list's region's `decref_point` at the
/// destructure's id (the post-pass that extends `decref_point` via
/// binding uses skips `r` when its regions are empty). The runtime
/// symptom (only with consumers that traverse the list via traits, like
/// `(println r)`) is an arena::deref panic on a stale ptr — but the
/// underlying analysis bug is independent of which consumer is used.
///
/// This test pins the analysis invariant directly: after region
/// inference, every destructured binding's `binding_source_regions`
/// must include the source value's region(s).
#[test]
fn letrec_init_does_not_overwrite_destructure_binding_regions() {
    use crate::symbol::SymbolTable;
    let mut symbols = SymbolTable::new();
    // Multi-form source. The top-level compile-file-to-fhir letrec
    // declares placeholder `r = nil` initializers for each pattern
    // binding introduced by destructures across the whole file —
    // those are the inits whose region contributions the letrec walk
    // must union with (not overwrite) the destructure-assigned
    // binding_regions[r].
    let source =
        "(begin (def (a & r) (list 1 2 3)) (length r)) (def (a b & r) (list 1 2)) (length r)";
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);
    // Find the pattern binding `r` introduced by the destructure.
    // The arena names it from the source symbol, so we look it up
    // by name. The letrec also declares it; the destructure's
    // pattern shares the same Binding id.
    let r_binding =
        find_binding_by_name(&hir, "r", &arena, &symbols).expect("expected binding `r`");
    let r_regions = info
        .binding_source_regions
        .get(&r_binding)
        .cloned()
        .unwrap_or_default();
    // Both `(list ...)` allocations feed `r` via destructure-rest
    // at the same Binding id. The letrec walk unions per-init
    // contributions so r tracks BOTH list regions. If the second
    // init's walk overwrote the first's contribution, r would track
    // only the latest list — so the FIRST list's region's
    // decref_point would be left at its destructure id and
    // `(length r)` inside the begin (which uses r while r still
    // points into the first list) would read a freed page.
    let list_calls = find_calls_to_primitive(&hir, "list", &arena, &symbols);
    assert_eq!(
        list_calls.len(),
        2,
        "expected exactly two (list ...) calls in the test source"
    );
    let list_regions: Vec<_> = list_calls
        .iter()
        .filter_map(|id| info.alloc_region.get(id).copied())
        .collect();
    let r_covers: Vec<_> = list_regions
        .iter()
        .filter(|lr| r_regions.contains(lr))
        .copied()
        .collect();
    assert_eq!(
        r_covers.len(),
        list_regions.len(),
        "r's binding_source_regions={:?} must include EVERY list region the \
             destructure could have bound it from; got list_regions={:?}, covered={:?}",
        r_regions,
        list_regions,
        r_covers,
    );
}
