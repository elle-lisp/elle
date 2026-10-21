// audited: 2026-10-21
//! Lowering's basic shapes, the ownership forest's adopts, `LoadSelf`, and the compiled forward cells a binder mints.
//!
//! docs/impl/lir.md
//! docs/impl/region/ownership.md

use super::*;

// ── Basic lowering smoke tests ───────────────────────────────────

#[test]
fn test_lower_int() {
    let arena = crate::hir::BindingArena::new();
    let mut lowerer = Lowerer::new(&arena).with_heap(test_heap());
    let hir = Hir::silent(HirKind::Int(42), make_span());
    let func = lowerer.lower(&hir).unwrap();
    assert!(func.entry.view().block_count() > 0);
}

#[test]
fn test_lower_if() {
    let arena = crate::hir::BindingArena::new();
    let mut lowerer = Lowerer::new(&arena).with_heap(test_heap());
    let hir = Hir::silent(
        HirKind::If {
            cond: Box::new(Hir::silent(HirKind::Bool(true), make_span())),
            then_branch: Box::new(Hir::silent(HirKind::Int(1), make_span())),
            else_branch: Box::new(Hir::silent(HirKind::Int(2), make_span())),
        },
        make_span(),
    );
    let func = lowerer.lower(&hir).unwrap();
    // An If lowers to four blocks: entry, then, else, merge
    assert_eq!(func.entry.view().block_count(), 4);
    // Entry block should have a Branch terminator
    assert!(matches!(
        func.entry.view().block(0).terminator(),
        Terminator::Branch { .. }
    ));
}

#[test]
fn test_lower_begin() {
    let arena = crate::hir::BindingArena::new();
    let mut lowerer = Lowerer::new(&arena).with_heap(test_heap());
    let hir = Hir::silent(
        HirKind::Begin(vec![
            Hir::silent(HirKind::Int(1), make_span()),
            Hir::silent(HirKind::Int(2), make_span()),
        ]),
        make_span(),
    );
    let func = lowerer.lower(&hir).unwrap();
    assert!(func.entry.view().block_count() > 0);
}

// ── Ownership forest: AdoptRegion emission ──
//
// The never-mergeable shape: a Fresh mutable container (`@array`) and the value
// pushed into it (`array`) are both call-result regions — no static slot, so
// MERGE cannot collapse them. The solver classifies `{container, value}` as an
// externally-unique Owned subtree and the lowerer emits `AdoptRegion(container,
// value)` at the push site (instead of the interior edge's inert IncrefRegion), so
// the container's free subtree-drops the value. The ownership forest is
// unconditional. A shape the inference cannot prove externally unique stays Shared
// and emits the per-region-RC baseline instead, so no `AdoptRegion` appears for it.

#[test]
fn adopt_region_emitted_for_owned_container() {
    // The push records a containment edge (value -> container); both are Fresh
    // call-results, so the subtree is Owned and the interior edge becomes an adopt.
    let module = compile_to_lir("(begin (%array-push (@array) (array 1 2)) nil)");
    assert!(
        count_adopt_regions(&module) >= 1,
        "the Owned container/value subtree must emit an AdoptRegion at the push \
         site; got {}",
        count_adopt_regions(&module),
    );
}

#[test]
fn adopt_region_emitted_per_member_for_interior_cycle() {
    // The shared-container cut: a Fresh container `root` directly holds two members
    // `a` and `b`, which reference each other (`a ⊇ b`, `b ⊇ a` — the interior cycle).
    // Each member is adopted DIRECTLY by the root, so TWO AdoptRegions are emitted (one
    // per member); the interior a↔b edges carry no adopt of their own. The push order
    // makes `root` outlive both members (its pushes come last), so the lifetime
    // obligation holds. Counterfactual: a flat cut that refuses the whole subtree
    // (an interior edge's target is a member, not the root) emits zero adopts.
    let module = compile_to_lir(
        "(begin (let [root (@array) a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) \
                         (%array-push root a) (%array-push root b) nil)) \
                nil)",
    );
    assert!(
        count_adopt_regions(&module) >= 2,
        "the shared-container cut must emit one AdoptRegion per member (>=2) for a root \
         holding an interior cycle; got {}",
        count_adopt_regions(&module),
    );
}

#[test]
fn adopt_region_emitted_for_deep_nesting_chain() {
    // Deep nesting: `root` holds `a` and `a` holds `b`, but `root` does NOT hold `b`
    // directly (`root ⊇ a ⊇ b`). `b` is adopted by its ACTUAL parent `a`, and `a` by the
    // root — TWO AdoptRegions forming a multi-level owner chain the root's recursive
    // subtree drop frees as a unit. Counterfactual: a flat cut that refuses the whole
    // subtree (`b` has no `member → root` edge) emits zero adopts.
    let module = compile_to_lir(
        "(begin (let [root (@array) a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push root a) nil)) \
                nil)",
    );
    assert!(
        count_adopt_regions(&module) >= 2,
        "deep nesting (root ⊇ a ⊇ b) must emit one AdoptRegion per non-root member (>=2): \
         `a` adopted by the root, `b` by its parent `a`; got {}",
        count_adopt_regions(&module),
    );
}

#[test]
fn adopt_region_emitted_for_captured_value() {
    // The capture cut: a pair `p` captured by a LOCAL closure `c` (called in place,
    // discarded). Tight last-use for a captured-and-owned value admits the Owned
    // subtree {closure, p}. The lowerer emits a value-resolved `AdoptRegion(closure, p)`
    // at the closure-construction site (MakeClosure), and the closure's subtree drop
    // then frees `p`. Capture records no `cross_region_refs` store site, so the adopt
    // rides the closure rather than a store node.
    let module =
        compile_to_lir("(begin (let [p (%pair 1 2)] (let [c (fn [] (length p))] (c))) nil)");
    assert!(
        count_adopt_regions(&module) >= 1,
        "the captured-value Owned subtree must emit an AdoptRegion at the closure \
         construction; got {}",
        count_adopt_regions(&module),
    );
}

#[test]
fn capture_adopt_reloads_upvalue_via_load_capture() {
    // The capture-adopt contract's env-reload half. A `capture_adopt_edges` entry
    // may name a captured binding that is an UPVALUE of the constructing function
    // (the enclosing lambda also captures it, forwarding). The lowerer emits it by
    // reloading the captured value from the closure environment (`LoadCapture`)
    // rather than a local slot. No shape's inference produces such an edge: the
    // lifetime obligation refuses a region-rooted upvalue owner, and the owner
    // that CAN hold one is the activation/fiber node. So the test INJECTS the edge
    // at the `make_lowerer_with` seam. It pins the emit capability the owner-node
    // cuts rely on ("suppress ⊆ adopt" held by capability, not refusal).
    //
    // Counterfactual: a lowerer with no env-reload emit leaves the injected upvalue
    // edge suppressed but unadopted, so the capture-adopt-contract debug_assert in
    // `lower_lambda_expr` panics. With the emit, the nested closure's construction
    // site carries a LoadCapture immediately feeding an AdoptRegion.
    let (mut lowerer, hir) = make_lowerer_with(
        "(begin (let [m (%pair 1 2)] \
           (let [outer (fn [] (let [o (fn [] (%first m))] (o)))] (outer))) nil)",
        |info, hir| {
            // m's region: the sole %pair allocation in the program.
            fn find_pair(h: &Hir, out: &mut Option<HirId>) {
                if let HirKind::Intrinsic { op, .. } = &h.kind {
                    if *op == crate::hir::IntrinsicOp::Pair && out.is_none() {
                        *out = Some(h.id);
                    }
                }
                h.for_each_child(|c| find_pair(c, out));
            }
            let mut pair_id = None;
            find_pair(hir, &mut pair_id);
            let m_r = *info
                .alloc_region
                .get(&pair_id.expect("a %pair node"))
                .expect("the pair has an alloc region");
            // o's Lambda: the DEEPEST lambda capturing a binding that holds m's
            // region — `outer` captures m to forward it (making o's capture an
            // upvalue); o is the nested capturer.
            fn deepest_capturing(
                h: &Hir,
                info: &crate::hir::region::RegionInfo,
                m: crate::hir::region::Region,
                depth: usize,
                best: &mut Option<(usize, HirId)>,
            ) {
                let child_depth = if let HirKind::Lambda(lambda) = &h.kind {
                    let captures_m = lambda.captures.iter().any(|c| {
                        info.binding_source_regions
                            .get(&c.binding)
                            .is_some_and(|rs| rs.contains(&m))
                    });
                    if captures_m && best.is_none_or(|(d, _)| depth > d) {
                        *best = Some((depth, h.id));
                    }
                    depth + 1
                } else {
                    depth
                };
                h.for_each_child(|c| deepest_capturing(c, info, m, child_depth, best));
            }
            let mut best = None;
            deepest_capturing(hir, info, m_r, 0, &mut best);
            let (_, o_id) = best.expect("a nested lambda capturing m");
            let o_r = *info
                .alloc_region
                .get(&o_id)
                .expect("the nested closure has an alloc region");
            // Mirror what the solver records for a capture adopt
            // (src/hir/region/infer/analyze/adopt.rs): the edge plus the
            // member's suppressed decref (reclaimed solely by the subtree drop).
            info.capture_adopt_edges
                .entry(o_id)
                .or_default()
                .push((m_r, o_r));
            info.suppressed_decref_regions.insert(m_r);
        },
    );
    let module = lowerer.lower(&hir).expect("lower");
    assert!(
        count_adopt_regions(&module) >= 1,
        "an injected upvalue capture-adopt edge must emit an AdoptRegion at the nested \
         closure's construction; got {}",
        count_adopt_regions(&module),
    );
    // The adopt's child operand is reloaded from the constructing function's ENV — a
    // LoadCapture immediately feeding the AdoptRegion — inside a closure body (outer's),
    // never the entry function.
    let env_reloaded_adopt = module.closures.iter().any(|f| {
        let instrs = flat_instrs(f);
        instrs.windows(2).any(|w| {
            matches!(
                w[0],
                InstrRef::LoadCapture { .. } | InstrRef::LoadCaptureRaw { .. }
            ) && matches!(w[1], InstrRef::AdoptRegion { .. })
        })
    });
    assert!(
        env_reloaded_adopt,
        "the upvalue capture adopt must reload the captured value via LoadCapture \
         (the env), immediately feeding the AdoptRegion, in the enclosing lambda's body",
    );
}

// ── Self-reference: LoadSelf resolves both positions ──
//
// A self-recursive closure's reference to ITSELF lowers to `LoadSelf`, never a
// capture-slot load of a forward cell. That holds in value position (`go` handed
// back, stored, or passed to a HOF) and in call position (`(go …)`). Both routes go
// through the one op, so each isolated shape below emits exactly one `LoadSelf`.

#[test]
fn value_position_self_reference_lowers_to_load_self() {
    // `go` referenced ONLY in value position — its whole body is `go`, handed back
    // as the result — and never self-called: exactly one LoadSelf, the returned `go`.
    let module = compile_to_lir("(letrec [go (fn [m] go)] (go 3))");
    assert_eq!(
        count_load_self(&module),
        1,
        "a value-position self-reference must lower to exactly one LoadSelf (the returned `go`)",
    );
}

#[test]
fn call_position_self_reference_lowers_to_load_self() {
    // `go` referenced ONLY in call position (`(go (%sub m 1))`), no value-position
    // self-reference. The self-CALL re-dispatches through the executing closure, so
    // its callee lowers to one LoadSelf — where a forward-cell callee load would emit
    // none. (`(go 3)` in the letrec body is not a self-reference: it is outside `go`.)
    let module = compile_to_lir("(letrec [go (fn [m] (if (%lt m 1) 0 (go (%sub m 1))))] (go 3))");
    assert_eq!(
        count_load_self(&module),
        1,
        "a call-position self-reference re-dispatches through the executing closure, lowering \
         its callee to exactly one LoadSelf",
    );
}

// ── Compiled forward cells: one mint, one address space ──────────
//
// A binding whose forward cell is COMPILED holds the cell in its OWN stack slot
// (`BindingInner::compiled_forward_cell`). So `populate_env` must not mint a
// shadow env cell over it, and every later recording of the binding's address
// space must say `Local`. Both halves read `compiled_cell_bindings`, which
// `allocate_compiled_cell_slot` writes as it mints the cell — the pairing the
// solver keeps in `record_compiled_cell`.
//
// The counter-factual: a binder form that minted the cell itself and registered
// nothing. The cell then sits in a stack slot while `value_slot_for` still reads
// the binding as an in-lambda upvalue, so the init's release loads an env index
// (`LoadCaptureRaw`) for a value that is on the stack.

#[test]
fn a_letrec_registers_every_compiled_forward_cell_it_mints() {
    let (mut lowerer, hir) = make_lowerer(
        "(letrec [ev (fn [n] (if (%lt n 1) :even (od (%sub n 1)))) \
                  od (fn [n] (if (%lt n 1) :odd (ev (%sub n 1))))] \
           (ev 4))",
    );
    let module = lowerer.lower(&hir).expect("lower");
    let cells = func_count(&module.entry, |i| {
        matches!(i, InstrRef::MakeCaptureCell { .. })
    });
    assert_eq!(
        cells, 2,
        "precondition: ev and od capture each other, so the letrec prebinds a \
         compiled forward cell for each",
    );
    assert_eq!(
        lowerer.compiled_cell_bindings.len(),
        cells,
        "every compiled cell a binder form mints registers its binding, so \
         `value_slot_for` names the stack slot the cell lives in",
    );
}
