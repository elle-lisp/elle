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
    // the shared-slot capture-cell leak; see docs/impl/region/model.md, "one
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

#[test]
fn walk_records_cell_contains_content_for_compiled_letrec_cell() {
    // A local letrec forward-reference cell holds its closure by a compiled
    // `MakeCaptureCell` (`leaf`, captured by `drive` before its initializer runs). The
    // walk must record `cell ⊇ content` in `containment_edges`, keyed at the letrec's
    // scope id, so external uniqueness sees the CELL as the container of the closure —
    // the uncounted cell store is otherwise invisible to the scan.
    //
    // Counter-factual: with no such edge the content region is a free-standing top
    // container the scan cannot bound through its cell (the invisible-containment hole
    // this modeling closes; region/adopt.md § "The capture adopt").
    let src = "(defn build [n] \
                 (letrec [drive (fn [m] (if (%le m 0) 0 (begin (leaf m) (drive (%sub m 1))))) \
                          leaf (fn [m] m)] \
                   (drive n)))";
    let (_hir, _arena, info) = pipeline(src);
    // At least one compiled cell exists (the `leaf` forward cell).
    let cells: Vec<(HirId, Binding, Region)> = info
        .begin_cell_regions
        .iter()
        .flat_map(|(&scope, v)| v.iter().map(move |&(b, r)| (scope, b, r)))
        .collect();
    assert!(
        !cells.is_empty(),
        "shape must mint at least one compiled capture cell (the leaf forward cell); \
         begin_cell_regions={:?}",
        info.begin_cell_regions,
    );
    // Every compiled cell's content is recorded contained in the cell.
    for (scope, b, cell_r) in cells {
        let contents = info
            .binding_source_regions
            .get(&b)
            .cloned()
            .unwrap_or_default();
        for content_r in contents {
            if content_r == cell_r {
                continue;
            }
            assert!(
                info.containment_edges.contains(&(scope, content_r, cell_r)),
                "cell r{} (binding {:?}) must contain its content r{} via a containment \
                 edge keyed at the scope @{}; containment_edges={:?}",
                cell_r.0,
                b,
                content_r.0,
                scope.0,
                info.containment_edges,
            );
        }
    }
}

#[test]
fn walk_records_no_cell_content_for_populate_env_route() {
    // An in-lambda captured MUTABLE local goes through the runtime `populate_env` env
    // cell (`StoreCapture`), not a compiled `MakeCaptureCell` — a phantom region the
    // runtime env owns. It mints NO `begin_cell_regions` entry, so the walk records NO
    // `cell ⊇ content` edge for it; the env cell stays a borrow (region/adopt.md
    // non-goal: only compiled cells get the edge).
    let src = "(defn mk [] (let [@x (%pair 1 2)] (fn [] x)))";
    let (_hir, _arena, info) = pipeline(src);
    // No compiled cell is minted for the env-cell route.
    assert!(
        info.begin_cell_regions.values().all(|v| v.is_empty()),
        "an in-lambda captured mutable local must NOT mint a compiled cell (populate_env \
         route); begin_cell_regions={:?}",
        info.begin_cell_regions,
    );
    // ...and no cell⊇content edge targets an env-cell phantom (a cell_release_region).
    for &(_, _, dst) in &info.containment_edges {
        assert!(
            !info.cell_release_regions.contains(&dst),
            "no cell⊇content edge may target an env-cell phantom r{} (the populate_env \
             route stays a borrow); containment_edges={:?}",
            dst.0,
            info.containment_edges,
        );
    }
}

#[test]
fn capture_edge_points_at_cell_region_not_content() {
    // §5b — the re-pointed capture edge. `drive` captures the letrec forward binding
    // `leaf` before its initializer runs, so `leaf` is a compiled `MakeCaptureCell`. The
    // capture records NO `cross_region_refs` edge (the RC double-count fix), so
    // `capture_containment_edges` re-derives it — and it must point at the CELL region,
    // not `leaf`'s content: paired with the walk's `cell ⊇ content` edge, external
    // uniqueness then sees the true chain `closure ⊇ cell ⊇ content`
    // (region/adopt.md § "The capture adopt"; the emit realizes it via `AdoptCellRegion`).
    //
    // Counter-factual: before the re-point, `capture_containment_edges` resolved a capture
    // through `binding_source_regions[leaf]` — the CONTENT region — so the edge read
    // `closure ⊇ content`, skipping the cell entirely and leaving the forest unable to
    // bound the content through its cell (the invisible-containment hole).
    let (_hir, _arena, info, edges) = capture_edges(
        "(defn build [n] \
           (letrec [drive (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             (%add (drive n) 0)))",
    );
    // The sole compiled cell is `leaf`'s forward cell.
    let cells: Vec<(Binding, Region)> = info
        .begin_cell_regions
        .values()
        .flatten()
        .copied()
        .collect();
    assert_eq!(
        cells.len(),
        1,
        "shape must mint exactly one compiled cell (leaf's forward cell); got {cells:?}",
    );
    let (leaf_binding, cell_r) = cells[0];
    // Its content region — the `(site, content, cell)` walk edge — is what the old
    // capture edge wrongly pointed at.
    let content_r = info
        .containment_edges
        .iter()
        .find(|&&(_, _, cell)| cell == cell_r)
        .map(|&(_, content, _)| content)
        .expect("the cell must carry a `cell ⊇ content` edge (§5a)");
    // The re-point resolves the cell through `single_cell_region_of`, the same gate the
    // lowerer uses — so analysis and emit name the same cell.
    assert_eq!(
        info.single_cell_region_of(leaf_binding),
        Some(cell_r),
        "single_cell_region_of(leaf) must name the sole compiled cell r{}",
        cell_r.0,
    );
    // The capture edge points at the CELL (r{cell_r}), for some capturing closure.
    let cell_edges: Vec<(HirId, Region, Region)> = edges
        .iter()
        .copied()
        .filter(|&(_, src, _)| src == cell_r)
        .collect();
    assert!(
        !cell_edges.is_empty(),
        "capture_containment_edges must carry a `closure ⊇ cell` edge for the cell r{} \
         (the capturing closure is `drive`); edges={edges:?}",
        cell_r.0,
    );
    // ...and NEVER at the content region (the pre-fix mis-pointing).
    assert!(
        !edges.iter().any(|&(_, src, _)| src == content_r),
        "no capture edge may point at leaf's CONTENT region r{} — that is the invisible- \
         containment mis-pointing the re-point corrects; edges={edges:?}",
        content_r.0,
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
