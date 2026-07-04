use super::*;

// ── Emit / yield ────────────────────────────────────────────────

// ── Region inference tests for unique-region default model ──────

#[test]
fn let_body_value_region_escapes_let_scope() {
    // `(fn () (let [x (string "a")] x))` — x's region's `decref_point`
    // is at the inner Var(x), NOT a Let HirId. A value's "scope" is
    // just its last-use HirId; the let does not own a region.
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (let [x (string \"a\")] x))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected one (string ...) call");
    let alloc = allocs[0];

    let region = info
        .alloc_region
        .get(&alloc)
        .copied()
        .expect("alloc must have a region");
    let region_data = info
        .region_data
        .get(&region)
        .unwrap_or_else(|| panic!("region r{} must have RegionData", region.0));

    let lets = find_lets(&hir);
    assert!(
        !lets.contains(&region_data.decref_point),
        "decref_point @{} must NOT be a Let HirId; Lets are {:?}",
        region_data.decref_point.0,
        lets,
    );
}

#[test]
fn yield_value_region_outlives_emit_scope() {
    // `(fn () (let [x (string "a")] (emit :yield x)))` — x's region's
    // `decref_point` is at the Emit node (the last use). The runtime
    // incref at handle_emit keeps the region alive past the matching
    // DecrefRegion at the resume site.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected one (string ...) call");
    let alloc = allocs[0];
    let emit = find_first_emit(&hir).expect("emit present");

    let region = info
        .alloc_region
        .get(&alloc)
        .copied()
        .expect("alloc must have a region");
    let region_data = info
        .region_data
        .get(&region)
        .unwrap_or_else(|| panic!("region r{} must have RegionData", region.0));

    assert_eq!(
        region_data.decref_point, emit,
        "yielded alloc @{} should have decref_point at Emit @{}, got @{}",
        alloc.0, emit.0, region_data.decref_point.0,
    );
}

#[test]
fn cross_region_edge_recorded_for_push() {
    // The %array-push primitive emits a cross-region edge entry
    // from the pushed value's region to the collection's value
    // region (NOT the collection's binding region — under
    // unique-per-alloc those are distinct).
    let (hir, arena, symbols, info) =
        analyze_with_hir("(let [acc @[] x (string \"a\")] (begin (%array-push acc x) acc))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected one (string ...) call");
    let x_alloc = allocs[0];
    let x_region = info.alloc_region.get(&x_alloc).copied().expect("x region");

    // Any edge whose source is x's region is a valid hit for this
    // test — the destination is the @[] allocation's region, which
    // we can't easily name without walking patterns. Asserting on
    // the source side alone is enough to prove the push intrinsic
    // produces an edge.
    let edges_from_x: Vec<_> = info
        .cross_region_refs
        .iter()
        .filter(|(_, src, _)| *src == x_region)
        .collect();
    assert!(
        !edges_from_x.is_empty(),
        "expected an edge from x's region r{} into a collection; got {:?}",
        x_region.0,
        info.cross_region_refs,
    );
}

#[test]
fn capture_records_no_cross_region_edge() {
    // RC double-counting fix: a closure capturing a heap value must
    // NOT record a static cross-region edge (the lowerer turns each
    // edge into an IncrefRegion). The runtime auto-incref over the
    // Closure env already pins the captured region —
    // `incref_cross_region_refs`/`find_object_cross_refs` covers
    // `Closure { env }` — and the cascade decref in `free_runtime_region_pages`
    // releases it when the closure region dies. A static
    // IncrefRegion here would be a *second* incref against that
    // *single* cascade decref: the captured region's RC never
    // returns to its pre-capture value, a per-iteration leak in
    // loops. The captured
    // pair is referenced nowhere but the closure, so the ONLY edge
    // that could originate from its region is the capture edge.
    let (hir, _arena, _symbols, info) = analyze_with_hir("(let [p (%pair 1 2)] (fn () p))");
    let pair_id = find_intrinsic_in_let(&hir, crate::hir::expr::IntrinsicOp::Pair)
        .expect("expected a %pair intrinsic in the let");
    let p_region = info
        .alloc_region
        .get(&pair_id)
        .copied()
        .expect("captured pair must have an alloc region");
    let edges_from_p: Vec<_> = info
        .cross_region_refs
        .iter()
        .filter(|(_, src, _)| *src == p_region)
        .collect();
    assert!(
        edges_from_p.is_empty(),
        "closure capture must not record a cross-region edge from the \
             captured value's region r{}; got {:?} — such edges become \
             redundant IncrefRegions that double-count against the runtime \
             auto-incref over the closure env",
        p_region.0,
        edges_from_p,
    );
}
