use super::*;

// ── Emit / yield ────────────────────────────────────────────────

// ── Region inference tests for unique-region default model ──────

#[test]
fn let_body_value_region_escapes_let_scope() {
    // `(fn () (let [x (string "a")] x))` — x's region's `decref_point`
    // is at the inner Var(x), NOT a Let HirId. A value's "scope" is
    // just its last-use HirId; the let does not own a region.
    let (hir, arena, _symbols, info) = analyze_with_hir("(fn () (let [x (string \"a\")] x))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena);
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

// ── which yields need a minted body reference ───────────────────────────────
//
// A park's escape retain is the DELIVERY reference the resumer's result release
// consumes; the reference a discarded fiber's discharge stands in for is the
// body's own, released past the suspend. So the question each `Emit` answers is
// whether its own body releases the payload anywhere
// (docs/impl/region/owner.md § "Park/unpark symmetry").

#[test]
fn body_allocated_yield_payload_is_not_borrowed() {
    // The body allocates what it yields, so its `decref_point` — the Emit node
    // itself, per `yield_value_region_outlives_emit_scope` above — sits in the
    // emitting lambda. That release is the one the discharge stands in for; a
    // minted second reference would be stranded at every abandoned park.
    let (hir, _arena, _symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    let emit = find_first_emit(&hir).expect("emit present");
    assert!(
        !info.borrowed_emit_payloads.contains(&emit),
        "a body-allocated yield payload owns its own reference; \
         borrowed_emit_payloads = {:?}, emit @{}",
        info.borrowed_emit_payloads,
        emit.0,
    );
}

#[test]
fn captured_yield_payload_is_borrowed() {
    // The value is allocated and released by the ENCLOSING lambda, so the
    // yielding body holds no reference of its own and the discharge would
    // release the resumer's.
    let (hir, _arena, _symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (fn () (emit :yield x))))");
    let emit = find_first_emit(&hir).expect("emit present");
    assert!(
        info.borrowed_emit_payloads.contains(&emit),
        "a yield of a captured value borrows it: the emitting body releases it \
         nowhere, so the mint is owed; borrowed_emit_payloads = {:?}, emit @{}",
        info.borrowed_emit_payloads,
        emit.0,
    );
}

#[test]
fn captured_parameter_yield_payload_is_borrowed() {
    // The everyday production shape: a fiber body closes over the enclosing
    // function's parameter and yields it. The parameter's own release belongs to
    // the frame the caller handed it to, and runs whatever the fiber does.
    let (hir, _arena, _symbols, info) = analyze_with_hir("(fn (p) (fn () (emit :yield p)))");
    let emit = find_first_emit(&hir).expect("emit present");
    assert!(
        info.borrowed_emit_payloads.contains(&emit),
        "a yield of a captured parameter borrows it; borrowed_emit_payloads = {:?}, emit @{}",
        info.borrowed_emit_payloads,
        emit.0,
    );
}

#[test]
fn own_parameter_yield_payload_is_not_borrowed() {
    // The contrast that keeps the question about the emitting BODY rather than
    // about the syntax: a body yielding its own parameter holds the reference the
    // caller moved in, and releases it at that parameter's last use — past the
    // suspend, which is exactly the release the discharge stands in for.
    let (hir, _arena, _symbols, info) = analyze_with_hir("(fn (p) (emit :yield p))");
    let emit = find_first_emit(&hir).expect("emit present");
    assert!(
        !info.borrowed_emit_payloads.contains(&emit),
        "a body owns the parameter it was handed; borrowed_emit_payloads = {:?}, emit @{}",
        info.borrowed_emit_payloads,
        emit.0,
    );
}

// ── the same question, asked of the emit OPERATION ──────────────────────────
//
// A first argument the compiler cannot read as a keyword set falls through to the
// `emit` primitive, so the park is an ordinary call and there is no `Emit` node to
// key on. The walk records the payload argument's regions against the CALL, so the
// borrowed-payload reading covers both shapes (docs/impl/region/owner.md
// § "What yields is the emit OPERATION, not the `Emit` node"). Both need the real
// classification: `emit_natives` is empty under the default one.

#[test]
fn dynamic_emit_of_a_captured_payload_is_borrowed() {
    // `s` makes the signal a runtime value, so `(emit s x)` is a Call. `x` belongs
    // to the enclosing lambda, so the emitting body releases it nowhere and the
    // discharge would release the resumer's reference.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(let [s :yield] (fn () (let [x (string \"a\")] (fn () (emit s x)))))");
    let calls = find_calls_to_primitive(&hir, "emit", &arena);
    assert_eq!(calls.len(), 1, "expected one dynamic (emit …) call");
    assert!(
        info.borrowed_emit_payloads.contains(&calls[0]),
        "a dynamic emit of a captured value borrows it: the emitting body releases \
         it nowhere, so the reference is owed; borrowed_emit_payloads = {:?}, call @{}",
        info.borrowed_emit_payloads,
        calls[0].0,
    );
}

#[test]
fn dynamic_emit_of_a_body_allocated_payload_is_not_borrowed() {
    // The contrast that keeps the reading about the emitting BODY rather than about
    // the call: the body allocates what it emits, so its `decref_point` sits in the
    // emitting lambda and a second reference would be stranded at every abandoned
    // park.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(let [s :yield] (fn () (let [x (string \"a\")] (emit s x))))");
    let calls = find_calls_to_primitive(&hir, "emit", &arena);
    assert_eq!(calls.len(), 1, "expected one dynamic (emit …) call");
    assert!(
        !info.borrowed_emit_payloads.contains(&calls[0]),
        "a body-allocated dynamic emit payload owns its own reference; \
         borrowed_emit_payloads = {:?}, call @{}",
        info.borrowed_emit_payloads,
        calls[0].0,
    );
}

#[test]
fn a_non_emit_delivers_call_records_no_emit_payload() {
    // The recording is keyed on the emit primitive, not on the `Delivers` effect it
    // shares with the other fiber value installers: `fiber/resume` hands its value
    // to a fiber that is already parked and yields nothing of its own, so naming its
    // argument here would mint a reference no park consumes.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn () (let [x (string \"a\")] (fn (h) (fiber/resume h x))))");
    let calls = find_calls_to_primitive(&hir, "fiber/resume", &arena);
    assert_eq!(calls.len(), 1, "expected one (fiber/resume …) call");
    assert!(
        !info.borrowed_emit_payloads.contains(&calls[0]),
        "only the emit primitive delivers a value the emitting body yields; \
         borrowed_emit_payloads = {:?}, call @{}",
        info.borrowed_emit_payloads,
        calls[0].0,
    );
}

#[test]
fn funnel_containment_recorded_for_push() {
    // `%array-push` compiles as a native funnel Call: the store is runtime-counted,
    // so the site records NO `cross_region_refs` edge (a compile-time IncrefRegion
    // would double-count against the collection's single free-time cascade decref).
    // The containment the ownership walks need is recovered structurally instead:
    // `containment_edges` carries `value → collection` keyed at the funnel call
    // site, and `funnel_store_sites` records the stored value for the compensate
    // gate. Needs the real classification so the callee resolves to its declared
    // `Funnel` effect and the `@[]` collection to its MutableArray RetType.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(let [acc @[] x (string \"a\")] (begin (%array-push acc x) acc))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena);
    assert_eq!(allocs.len(), 1, "expected one (string ...) call");
    let x_alloc = allocs[0];
    let x_region = info.alloc_region.get(&x_alloc).copied().expect("x region");

    assert!(
        !info
            .cross_region_refs
            .iter()
            .any(|(_, src, _)| *src == x_region),
        "a funnel store must record no cross_region_refs edge from the pushed \
         value's region r{}; got {:?}",
        x_region.0,
        info.cross_region_refs,
    );
    // Any containment edge whose source is x's region is a valid hit — the
    // destination is the @[] allocation's region, which we can't easily name
    // without walking patterns. The source side alone proves the push recovers
    // its containment.
    assert!(
        info.containment_edges
            .iter()
            .any(|&(_, src, _)| src == x_region),
        "the funnel recovery must record a containment edge from the pushed \
         value's region r{} into the collection; got {:?}",
        x_region.0,
        info.containment_edges,
    );
    assert!(
        info.funnel_store_sites
            .values()
            .flatten()
            .any(|&r| r == x_region),
        "the funnel store site must record the stored value's region r{}; got {:?}",
        x_region.0,
        info.funnel_store_sites,
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
