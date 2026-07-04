use super::*;

// ── "Every region corresponds to a real allocation" tests ──────
//
// These pin the rule documented in docs/impl/region-rules.md § "Every region
// must correspond to a real allocation": the regions walk must
// NOT call alloc_here at HIR nodes the lowerer is transparent for
// (MakeCell, DerefCell, SetCell) and MUST register call-shaped
// results (Call, Eval) in call_result_regions so the lowerer can
// emit value-gated DecrefValueRegion. Failure mode that the
// fixes prevent: a region exists at compile time with no matching
// alloc_in_region in the bytecode; its DecrefRegion at decref_point
// decrements an RC that no IncrefRegion ever raised, producing
// underflow or conflation with neighbouring region IDs.

#[test]
fn makecell_walk_is_transparent_pass_through() {
    // No pass in the current pipeline actually constructs
    // HirKind::MakeCell nodes — functionalize's Let/Letrec/Define
    // handlers emit cells implicitly (the lowerer's MakeCaptureCell
    // path at the binding site does the real allocation), and the
    // only MakeCell match arm in functionalize itself just
    // preserves nodes that arrived already wrapped. The variant
    // exists for future use and as a marker the lowerer recognizes
    // via lower_make_cell (transparent: delegates to lower_expr).
    //
    // Test the walk arm directly by synthesizing a tiny HIR with a
    // MakeCell at its root: assert that the walk produces NO
    // alloc_region entry for the MakeCell node and that it passes
    // through the value's regions (Vec::new() for an Int literal).
    use crate::hir::expr::{Hir, HirKind};
    use crate::syntax::Span;
    let arena = BindingArena::new();
    let span = Span::synthetic();
    let value = Hir::silent(HirKind::Int(42), span.clone());
    let mc = Hir::silent(
        HirKind::MakeCell {
            value: Box::new(value),
        },
        span,
    );
    let info = analyze_regions(&mc, &arena);
    assert!(
            !info.alloc_region.contains_key(&mc.id),
            "MakeCell @{} must not have an alloc_region entry — transparent at the lowerer; cell lives at the Let's MakeCaptureCell",
            mc.id.0,
        );
}

#[test]
fn derefcell_does_not_get_an_alloc_region() {
    // Same program. DerefCell wraps every read of x inside the
    // lambda body. DerefCell is transparent at the lowerer
    // (lower_var auto-unwraps via LoadCapture); the regions walk
    // must not manufacture a region for it.
    let (hir, _arena, _symbols, info) = analyze_with_hir("(let [@x 0] (fn () (assign x (+ x 1))))");
    let dc_ids = find_all(&hir, |h| matches!(&h.kind, HirKind::DerefCell { .. }));
    assert!(!dc_ids.is_empty(), "expected a DerefCell node in the HIR");
    for id in &dc_ids {
        assert!(
            !info.alloc_region.contains_key(id),
            "DerefCell @{} must not have an alloc_region entry — it's transparent at the lowerer",
            id.0,
        );
    }
}

#[test]
fn eval_is_registered_in_call_result_regions() {
    // Eval's result lives in a region the outer compilation didn't
    // allocate (it comes from the inner compilation's runtime).
    // The walk allocates a placeholder region for the Eval node
    // AND registers it in call_result_regions so the lowerer
    // emits DecrefValueRegion (value-gated) instead of
    // DecrefRegion (id-based).
    let (hir, _arena, _symbols, info) = analyze_with_hir("(eval 1)");
    let eval_id = find_first(&hir, |h| matches!(&h.kind, HirKind::Eval { .. }))
        .expect("expected an Eval node in the HIR");
    let eval_region = *info
        .alloc_region
        .get(&eval_id)
        .expect("Eval node must have a placeholder alloc_region");
    assert!(
            info.call_result_regions.contains(&eval_region),
            "Eval's placeholder region r{} must be in call_result_regions so the lowerer emits DecrefValueRegion (value-gated); got {:?}",
            eval_region.0,
            info.call_result_regions,
        );
}

#[test]
fn get_intrinsic_passes_through_collection_region() {
    // %get returns a value already living in the collection's
    // region (or one of its referent regions, for heap-valued
    // entries). The walk must pass through arg_regions[0] rather
    // than manufacturing a fresh region with no allocation.
    let (hir, _arena, _symbols, info) = analyze_with_hir("(let [s (string \"x\")] (%get s 0))");
    let get_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Get,
                ..
            }
        )
    })
    .expect("expected a %get node");
    assert!(
        !info.alloc_region.contains_key(&get_id),
        "%get @{} must not manufacture an alloc_region — pass through arg[0]",
        get_id.0,
    );
}

#[test]
fn put_intrinsic_gets_a_call_result_region() {
    // %put is a conditionally-allocating native: a mutable arg is mutated in
    // place (pass-through), an immutable arg yields a fresh copy. Like a Call,
    // its result gets its OWN call-result region — the handler mints a fresh
    // region and pass-through-retains, and the lowerer emits a value-based
    // `DecrefValueRegion` at the decref_point (freeing the minted region for a
    // fresh copy, or arg 0's region for the in-place case). It is NOT
    // region-transparent. (Counterfactual: were %put to pass through arg[0] and
    // manufacture no region, the freshly-copied immutable result would have no
    // region of its own to release — it must be born in its own call-result
    // region so the lowerer can free it.)
    let (hir, _arena, _symbols, info) = analyze_with_hir("(let [m @{:a 1}] (%put m :b 2))");
    let put_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Put,
                ..
            }
        )
    })
    .expect("expected a %put node");
    let put_region = info
        .alloc_region
        .get(&put_id)
        .copied()
        .unwrap_or_else(|| panic!("%put @{} must manufacture a call-result region", put_id.0));
    assert!(
        info.call_result_regions.contains(&put_region),
        "%put's region r{} must be a call-result region (value-based DecrefValueRegion), \
         like a native call — not a static-slot alloc",
        put_region.0,
    );
}

#[test]
fn typeof_and_length_have_no_region() {
    // %type-of returns an interned keyword; %length returns an
    // immediate int. Neither needs a region. The walk returns
    // Vec::new() for them — no alloc_region entry.
    let (hir, _arena, _symbols, info) =
        analyze_with_hir("(let [s (string \"abc\")] (begin (%length s) (%type-of s)))");
    let length_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Length,
                ..
            }
        )
    })
    .expect("expected a %length node");
    let typeof_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::TypeOf,
                ..
            }
        )
    })
    .expect("expected a %type-of node");
    assert!(
        !info.alloc_region.contains_key(&length_id),
        "%length must not manufacture an alloc_region"
    );
    assert!(
        !info.alloc_region.contains_key(&typeof_id),
        "%type-of must not manufacture an alloc_region"
    );
}

#[test]
fn freeze_and_thaw_get_a_real_region() {
    // %freeze and %thaw produce a new heap copy. Their lowering
    // uses emit_alloc, so the regions walk must assign each its
    // own alloc_region. (This complements the negative tests
    // above: these two intrinsics ARE allocating.)
    let (hir, _arena, _symbols, info) =
        analyze_with_hir("(let [m @[1 2]] (let [f (%freeze m)] (%thaw f)))");
    let freeze_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Freeze,
                ..
            }
        )
    })
    .expect("expected a %freeze node");
    let thaw_id = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Thaw,
                ..
            }
        )
    })
    .expect("expected a %thaw node");
    assert!(
        info.alloc_region.contains_key(&freeze_id),
        "%freeze must have an alloc_region — it really allocates"
    );
    assert!(
        info.alloc_region.contains_key(&thaw_id),
        "%thaw must have an alloc_region — it really allocates"
    );
}
