use super::*;

// ── The coalescing staticness predicate ─────────────────────────────────────
//
// `coalescible_region` / `coalescible_solver_region` (src/lir/lower/regionemit.rs)
// classify whether a value's region-RC mint can be slot-resolved (the value is a
// fresh local allocation whose region is statically nameable) or must stay
// value-resolved (the dynamic boundary). The predicate is computed and logged
// under --trace=rc; emission consumes it without changing behavior.
//
// These pins are written from docs/impl/region-rules.md § "Compile-time region
// selection (coalescing)", NOT from emission output: each is counterfactual
// against a predicate that misclassifies the case (an accept-all-live predicate
// accepts every "refused" pin; a reject-all predicate refuses every "accepted"
// pin).

#[test]
fn coalescible_predicate_class_logic() {
    // The predicate's class logic in isolation, over a hand-built `RegionInfo`.
    // This pins every dynamic-class exclusion — including the reassign classes
    // (`suppressed_decref_regions`, `mutated_binding_value_regions`) that
    // functionalize's assignment-conversion makes unreachable from straight-line
    // Elle source in this unit harness — exactly per docs/impl/region-rules.md
    // § "Compile-time region selection (coalescing)": Some iff the region is `live`
    // and in NONE of the four dynamic classes, and (for a returned `Var`)
    // `binding_source_regions` names exactly one region.
    use crate::hir::region::Region;
    let arena = crate::hir::BindingArena::new();

    // A returned-Var binding (not in the arena — the predicate only reads its id
    // as a `binding_source_regions` key) and a direct-allocation value node.
    let b = crate::hir::Binding(0);
    let var = Hir::silent(HirKind::Var(b), make_span());
    let alloc_node = Hir::silent(HirKind::Int(0), make_span());

    let target = Region(100);

    // Build a RegionInfo whose `target` region is `live` and placed in the named
    // dynamic class (or none). The returned-Var path keys `binding_source_regions`
    // on `b`; the direct-alloc path keys `alloc_region` on `alloc_node.id`.
    let build = |class: Option<&str>, live: bool, sources: Vec<Region>| {
        let mut info = RegionInfo::empty();
        if live {
            info.live_regions.insert(target);
        }
        info.binding_source_regions.insert(b, sources);
        info.alloc_region.insert(alloc_node.id, target);
        match class {
            Some("call_result") => {
                info.call_result_regions.insert(target);
            }
            Some("cell_release") => {
                // cell_release ⊆ call_result by construction; place it in both,
                // as the solver does, so the exclusion is exercised on its own merit.
                info.call_result_regions.insert(target);
                info.cell_release_regions.insert(target);
            }
            Some("suppressed") => {
                info.suppressed_decref_regions.insert(target);
            }
            Some("mutated") => {
                info.mutated_binding_value_regions.insert(target);
            }
            _ => {}
        }
        info
    };

    // Returned-Var path. Clean, single, live → accepted (and names `target`).
    let lw = |info| Lowerer::new(&arena).with_region_info(info);
    assert_eq!(
        lw(build(None, true, vec![target])).coalescible_solver_region(&var),
        Some(target),
        "a returned Var whose single source region is a live, non-dynamic local \
         allocation must coalesce",
    );
    // Each dynamic class refuses.
    for class in ["call_result", "cell_release", "suppressed", "mutated"] {
        assert_eq!(
            lw(build(Some(class), true, vec![target])).coalescible_solver_region(&var),
            None,
            "a returned Var whose region is in {class} must NOT coalesce (the \
             dynamic boundary, docs/impl/region-rules.md)",
        );
    }
    // Not live (a phantom region — e.g. a param's placeholder) refuses.
    assert_eq!(
        lw(build(None, false, vec![target])).coalescible_solver_region(&var),
        None,
        "a non-live (phantom) region must NOT coalesce",
    );
    // A branch-dependent mix (more than one source region) is not statically
    // nameable → refused, even when both regions are clean and live.
    let mut multi = build(None, true, vec![target, Region(101)]);
    multi.live_regions.insert(Region(101));
    assert_eq!(
        lw(multi).coalescible_solver_region(&var),
        None,
        "a returned Var with more than one source region (a branch mix) is not \
         statically nameable and must NOT coalesce",
    );
    // No source region at all (an immediate-valued binding) refuses.
    assert_eq!(
        lw(build(None, true, vec![])).coalescible_solver_region(&var),
        None,
        "a returned Var with no source region must NOT coalesce",
    );

    // Direct-allocation path (a non-Var value node, region read from `alloc_region`).
    assert_eq!(
        lw(build(None, true, vec![])).coalescible_solver_region(&alloc_node),
        Some(target),
        "a direct fresh allocation whose region is live and non-dynamic must coalesce",
    );
    assert_eq!(
        lw(build(Some("call_result"), true, vec![])).coalescible_solver_region(&alloc_node),
        None,
        "a direct allocation whose region is a call-result placeholder must NOT coalesce",
    );
    // No alloc_region entry for the node → refused.
    let nameless = Hir::silent(HirKind::Int(0), make_span());
    assert_eq!(
        lw(build(None, true, vec![])).coalescible_solver_region(&nameless),
        None,
        "a value node with no alloc_region entry must NOT coalesce",
    );
    // `target` and `var` are exercised by the accept/refuse assertions above; the
    // `&self` slot wrapper (class logic + emitted-alloc guard) is pinned by
    // `coalescible_region_requires_locally_emitted_slot`.
    let _ = (&target, &var);
}

#[test]
fn coalescible_region_requires_locally_emitted_slot() {
    // `coalescible_region` (the slot wrapper consumed by `lower_return`) layers a
    // runtime-population guard over the class predicate: the region's slot must be
    // mapped (`region_to_table`) AND stamped by an allocation emitted in THIS
    // function (`emitted_alloc_regions`), so the activation map populates it at
    // runtime. A value whose region is class-coalescible yet allocated in another
    // activation — an immutable captured upvalue, a `sys/spawn-vm` thunk returning
    // a captured string (tests/elle/concurrency.lisp) — has no slot stamped here,
    // so a slot-resolved `IncrefRegion` would resolve to None and free a live
    // region. Counterfactual: a wrapper that minted on demand (or omitted the
    // emitted-alloc guard) would accept the unstamped region — `AssertRegionMatches`
    // then detonates on the spawned thread.
    use crate::hir::region::{Region, StaticRegion};
    let arena = crate::hir::BindingArena::new();
    let alloc_node = Hir::silent(HirKind::Int(0), make_span());
    let target = Region(100);
    let mut info = RegionInfo::empty();
    info.live_regions.insert(target);
    info.alloc_region.insert(alloc_node.id, target);

    let mut l = Lowerer::new(&arena).with_region_info(info);
    // The class predicate accepts `target` (live, non-dynamic) …
    assert_eq!(
        l.coalescible_solver_region(&alloc_node),
        Some(target),
        "class predicate must accept a live, non-dynamic local region",
    );
    // … but the slot wrapper refuses until the region's slot is mapped …
    assert_eq!(
        l.coalescible_region(&alloc_node),
        None,
        "a class-coalescible region with no mapped slot (not allocated here) must \
         be refused — its activation slot would never be populated",
    );
    let slot = StaticRegion::new(7).expect("nonzero slot");
    l.region_to_table.insert(target, slot);
    // … and refuses a mapped-but-unstamped (phantom) slot …
    assert_eq!(
        l.coalescible_region(&alloc_node),
        None,
        "a mapped-but-unstamped slot (no emitted alloc) must be refused — the \
         phantom-region guard, mirroring emit_decref_region",
    );
    l.emitted_alloc_regions.insert(slot);
    // … and accepts once the slot is both mapped and locally stamped.
    assert_eq!(
        l.coalescible_region(&alloc_node),
        Some(slot),
        "a region whose slot is mapped AND emitted in this function coalesces",
    );
}

#[test]
fn coalescible_accepts_returned_fresh_pair() {
    // `(fn () (%pair 1 2))` — the tail is a fresh `%pair` allocation wrapped in
    // `Return`. Its region is a real local allocation (walk.rs `op.allocates()`),
    // so the return mint is coalescible (docs/impl/region-rules.md § "Compile-time
    // region selection (coalescing)", the direct-alloc branch). Counterfactual: a
    // reject-all predicate refuses it.
    let (lowerer, hir) = make_lowerer("(fn () (%pair 1 2))");
    let mut found = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if matches!(
            &v.kind,
            HirKind::Intrinsic {
                op: crate::hir::IntrinsicOp::Pair,
                ..
            }
        ) {
            found = true;
            assert!(
                lowerer.coalescible_solver_region(v).is_some(),
                "a returned fresh %pair allocation must coalesce",
            );
        }
    }
    assert!(
        found,
        "expected a returned %pair node in (fn () (%pair 1 2))"
    );
}

#[test]
fn coalescible_accepts_returned_var_bound_to_fresh_pair() {
    // `(fn () (let [p (%pair 1 2)] p))` — the tail is `p`, a `Var` whose single
    // source region is the fresh `%pair`. The returned-Var branch resolves it
    // through `binding_source_regions`. Counterfactual: a reject-all predicate
    // refuses it; a predicate that ignored `binding_source_regions` (only checked
    // `alloc_region` on the Var node, which has none) would also refuse.
    let (lowerer, hir) = make_lowerer("(fn () (let [p (%pair 1 2)] p))");
    let coalescible_var = return_value_ptrs(&hir).into_iter().any(|p| {
        let v = unsafe { &*p };
        matches!(&v.kind, HirKind::Var(_)) && lowerer.coalescible_solver_region(v).is_some()
    });
    assert!(
        coalescible_var,
        "a returned Var bound to a fresh %pair must coalesce via binding_source_regions",
    );
}

#[test]
fn coalescible_accepts_returned_string_literal() {
    // `(fn () "hi")` — a returned string literal is a `MaterializeConst`
    // allocation with its own live region (walk.rs String arm), so coalescible.
    let (lowerer, hir) = make_lowerer("(fn () \"hi\")");
    let mut found = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if matches!(&v.kind, HirKind::String(_)) {
            found = true;
            assert!(
                lowerer.coalescible_solver_region(v).is_some(),
                "a returned string literal must coalesce",
            );
        }
    }
    assert!(found, "expected a returned String node in (fn () \"hi\")");
}

#[test]
fn coalescible_refuses_returned_param() {
    // `(fn (x) x)` — the tail returns the fixed parameter `x`. A param's region
    // is a phantom call-result placeholder (walk.rs: `call_result_regions`, no
    // `alloc_here`, so not `live`), the dynamic boundary's "phantom param" row.
    // Counterfactual: a predicate omitting the live/call-result checks would
    // wrongly accept the param.
    let (lowerer, hir) = make_lowerer("(fn (x) x)");
    let mut found = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if let HirKind::Var(b) = &v.kind {
            if matches!(
                lowerer.arena.get(*b).scope,
                crate::hir::arena::BindingScope::Parameter
            ) {
                found = true;
                assert!(
                    lowerer.coalescible_solver_region(v).is_none(),
                    "a returned fixed parameter must NOT coalesce (phantom call-result region)",
                );
            }
        }
    }
    assert!(found, "expected a returned parameter Var in (fn (x) x)");
}

#[test]
fn coalescible_refuses_returned_opaque_call_result() {
    // `(fn (x) (let [t (f x)] t))` — `t` binds an opaque user-fn call result, a
    // `call_result_regions` placeholder (the caller cannot name the callee's
    // region — prediction-free). The returned `Var(t)` must be refused.
    let (lowerer, hir) = make_lowerer("(fn (x) (let [t (f x)] t))");
    let info_has = |b: &crate::hir::Binding, l: &Lowerer| {
        l.region_info
            .binding_source_regions
            .get(b)
            .is_some_and(|rs| {
                rs.iter()
                    .any(|r| l.region_info.call_result_regions.contains(r))
            })
    };
    let mut found = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if let HirKind::Var(b) = &v.kind {
            if info_has(b, &lowerer) {
                found = true;
                assert!(
                    lowerer.coalescible_solver_region(v).is_none(),
                    "a returned Var bound to an opaque call result must NOT coalesce",
                );
            }
        }
    }
    assert!(
        found,
        "expected a returned Var whose source is a call-result region in (f x)",
    );
}

#[test]
fn coalescible_refuses_returned_passthrough_call_result() {
    // `(let [id (fn (a) a)] (fn (x) (let [t (id x)] t)))` — `id` is an inlinable
    // pass-through; the solver resolves `(id x)` to `x`'s region, a param phantom
    // (call_result, not live). The returned `Var(t)` must be refused — the
    // dynamic boundary's pass-through row.
    let (lowerer, hir) = make_lowerer("(let [id (fn (a) a)] (fn (x) (let [t (id x)] t)))");
    // The interesting return is `t`, bound to `(id x)`: the solver inlines `id`
    // and resolves the result to `x`'s param region — a call-result placeholder.
    let is_call_result_sourced = |b: &crate::hir::Binding, l: &Lowerer| {
        l.region_info
            .binding_source_regions
            .get(b)
            .is_some_and(|rs| {
                rs.iter()
                    .any(|r| l.region_info.call_result_regions.contains(r))
            })
    };
    let mut checked = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if let HirKind::Var(b) = &v.kind {
            if is_call_result_sourced(b, &lowerer) {
                checked = true;
                assert!(
                    lowerer.coalescible_solver_region(v).is_none(),
                    "a returned Var bound to a pass-through call result must NOT coalesce",
                );
            }
        }
    }
    assert!(
        checked,
        "expected a returned Var whose source is a call-result region in the pass-through shape",
    );
}

#[test]
fn coalescible_refuses_returned_captured_upvalue() {
    // `(fn (@x) (g (fn () x)))` — the inner closure returns `x`, a captured
    // mutable param. Its region is a phantom call-result / cell placeholder (not
    // live), the dynamic boundary's "capture cell" row. The returned upvalue must
    // be refused. The lexical-capture proxy `is_captured` is module-private to
    // `hir`, so identify the captured upvalue structurally — the bindings some
    // closure captures, read from the lambda capture sets in the HIR.
    let (lowerer, hir) = make_lowerer("(fn (@x) (g (fn () x)))");
    fn captured_in(h: &Hir, out: &mut std::collections::HashSet<crate::hir::Binding>) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            for c in captures {
                out.insert(c.binding);
            }
        }
        h.for_each_child(|c| captured_in(c, out));
    }
    let mut captured = std::collections::HashSet::new();
    captured_in(&hir, &mut captured);
    let mut found = false;
    for p in return_value_ptrs(&hir) {
        let v = unsafe { &*p };
        if let HirKind::Var(b) = &v.kind {
            if captured.contains(b) {
                found = true;
                assert!(
                    lowerer.coalescible_solver_region(v).is_none(),
                    "a returned captured upvalue must NOT coalesce",
                );
            }
        }
    }
    assert!(
        found,
        "expected a returned captured upvalue in (fn (@x) (g (fn () x)))"
    );
}

#[test]
fn region_table_entries_are_static_regions_at_least_two() {
    // A function's `region_table` is `Vec<StaticRegion>`, not a bare
    // `Vec<u32>`. Every slot the lowerer mints into a table comes from
    // `new_static_region()` (≥ 2); ids 0 and 1 are reserved and never minted
    // into a function's table. Asserting `&Vec<StaticRegion>` plus `.get() >= 2`
    // pins both the type and the value invariant.
    //
    // Counterfactual: a bare `Vec<u32>` (a `RegionId` alias) would let neither
    // the `&Vec<StaticRegion>` binding nor `.get()` on an entry compile, so the
    // type itself enforces this.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] x))");
    let mut tables: Vec<&Vec<StaticRegion>> = vec![&module.entry.region_table];
    for c in &module.closures {
        tables.push(&c.region_table);
    }
    let mut total = 0;
    for table in tables {
        for sr in table {
            assert!(
                sr.get() >= 2,
                "function region_table slot must be >= 2, got {}",
                sr.get(),
            );
            total += 1;
        }
    }
    assert!(
        total >= 1,
        "expected the string-literal allocation to populate a region_table slot",
    );
}
