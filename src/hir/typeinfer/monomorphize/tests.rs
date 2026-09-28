// audited: 2026-09-28
//! Pins cross-unit monomorphization: a stdlib store wrapper called on a proven container collapses to its monomorphic op.
//!
//! docs/impl/dissolution.md

use crate::hir::arena::BindingArena;
use crate::hir::expr::{Hir, HirKind};

/// Collect the name of every call callee (through the ANF/`Var` wrappers) in
/// the tree, so a test can assert which ops a source form lowered to.
fn callee_names(h: &Hir, arena: &BindingArena, out: &mut Vec<String>) {
    if let HirKind::Call { func, .. } = &h.kind {
        if let Some(b) = super::unwrap_callee_binding(func) {
            if let Some(n) = crate::primitives::registration::static_name(arena.get(b).name) {
                out.push(n.to_string());
            }
        }
    }
    h.for_each_child(|c| callee_names(c, arena, out));
}

/// Cross-unit dispatch-wrapper monomorphization: a user call to the stdlib
/// `put` wrapper on a statically-proven `:struct` must collapse to the direct
/// `%put-struct` op — even though `put`'s definition lives in the stdlib
/// compile unit, not the caller's. The counter-factual: without the cross-unit
/// registry the call stays a `put` wrapper call, and its owned-param container
/// reference strands one region per call (the `native-tail-put-struct` row of
/// tests/impl/probe/stdlib.lisp).
#[test]
fn cross_unit_put_on_proven_struct_monomorphizes() {
    let mut rt = crate::runtime::Runtime::new(); // stdlib loaded
    let (_vm, symbols, cctx) = rt.parts();
    let (hir, arena) =
        crate::pipeline::compile_file_to_fhir("(put {:a 1} :b 2)", symbols, cctx, "<test>")
            .expect("compile");
    let mut callees = Vec::new();
    callee_names(&hir, &arena, &mut callees);
    assert!(
        callees.iter().any(|n| n == "%put-struct"),
        "a `put` on a proven :struct must collapse to %put-struct; callees were {:?}",
        callees,
    );
    assert!(
        !callees.iter().any(|n| n == "put"),
        "the polymorphic `put` wrapper call must be gone after monomorphization; \
         callees were {:?}",
        callees,
    );
}

/// The store family beyond `put`: `push`/`add` on a proven immutable container
/// collapse cross-unit to their monomorphic op the same way, through the same
/// registry with no per-op change. Guards that the mechanism is generic over the
/// store wrappers, not special-cased to `put`.
#[test]
fn cross_unit_push_add_on_proven_immutable_monomorphize() {
    let cases = [
        ("(push [1 2] 3)", "%push-array", "push"),
        ("(add (set 1 2) 3)", "%add-set", "add"),
        ("(push \"ab\" \"c\")", "%string-push", "push"),
    ];
    for (src, want_op, wrapper) in cases {
        let mut rt = crate::runtime::Runtime::new();
        let (_vm, symbols, cctx) = rt.parts();
        let (hir, arena) =
            crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>").expect("compile");
        let mut callees = Vec::new();
        callee_names(&hir, &arena, &mut callees);
        assert!(
            callees.iter().any(|n| n == want_op),
            "{src} must collapse to {want_op}; callees were {callees:?}",
        );
        assert!(
            !callees.iter().any(|n| n == wrapper),
            "the `{wrapper}` wrapper call must be gone in {src}; callees were {callees:?}",
        );
    }
}
