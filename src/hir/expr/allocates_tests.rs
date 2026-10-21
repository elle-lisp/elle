// audited: 2026-10-21
//! Which HIR node kinds `Hir::allocates` counts as producing a fresh heap value at their own id.
//!
//! docs/impl/anf.md

use super::*;
use crate::hir::binding::Binding;
use crate::hir::pattern::{HirPattern, PatternKey};

fn nil() -> Hir {
    Hir::silent(HirKind::Nil, Span::synthetic())
}

fn int(n: i64) -> Hir {
    Hir::silent(HirKind::Int(n), Span::synthetic())
}

#[test]
fn literals_do_not_allocate() {
    assert!(!nil().allocates());
    assert!(!int(0).allocates());
    assert!(!Hir::silent(HirKind::Bool(true), Span::synthetic()).allocates());
    assert!(!Hir::silent(HirKind::String("x".to_string()), Span::synthetic()).allocates());
}

#[test]
fn var_does_not_allocate() {
    assert!(!Hir::silent(HirKind::Var(Binding(0)), Span::synthetic()).allocates());
}

#[test]
fn call_allocates() {
    let call = Hir::silent(
        HirKind::Call {
            func: Box::new(nil()),
            args: vec![],
            is_tail: false,
        },
        Span::synthetic(),
    );
    assert!(call.allocates());
}

#[test]
fn eval_allocates() {
    let e = Hir::silent(
        HirKind::Eval {
            expr: Box::new(nil()),
            env: Box::new(nil()),
        },
        Span::synthetic(),
    );
    assert!(e.allocates());
}

#[test]
fn emit_allocates() {
    // The resume value takes the `Emit`'s place, and `lower_emit` mints a
    // reference for it that only a named slot can release.
    let e = Hir::silent(
        HirKind::Emit {
            signal: crate::signals::SIG_YIELD,
            value: Box::new(int(1)),
        },
        Span::synthetic(),
    );
    assert!(e.allocates());
}

#[test]
fn lambda_allocates() {
    let lambda = Lambda::new(vec![], 0, None, VarargKind::List, nil());
    let lam = Hir::silent(HirKind::Lambda(Box::new(lambda)), Span::synthetic());
    assert!(lam.allocates());
}

#[test]
fn pair_intrinsic_allocates() {
    let pair = Hir::silent(
        HirKind::Intrinsic {
            op: IntrinsicOp::Pair,
            args: vec![int(1), int(2)],
        },
        Span::synthetic(),
    );
    assert!(pair.allocates());
}

#[test]
fn non_allocating_intrinsic_does_not() {
    let add = Hir::silent(
        HirKind::Intrinsic {
            op: IntrinsicOp::Add,
            args: vec![int(1), int(2)],
        },
        Span::synthetic(),
    );
    assert!(!add.allocates());
}

#[test]
fn if_does_not_allocate_itself() {
    // An If produces no fresh heap value at its own HirId. What its
    // branches allocate, the ANF lift names inside the branches.
    let i = Hir::silent(
        HirKind::If {
            cond: Box::new(nil()),
            then_branch: Box::new(nil()),
            else_branch: Box::new(nil()),
        },
        Span::synthetic(),
    );
    assert!(!i.allocates());
}

#[test]
fn let_does_not_allocate_itself() {
    let l = Hir::silent(
        HirKind::Let {
            bindings: vec![(Binding(0), int(1))],
            body: Box::new(nil()),
        },
        Span::synthetic(),
    );
    assert!(!l.allocates());
}

#[test]
fn match_without_alloc_pattern_does_not_allocate() {
    let m = Hir::silent(
        HirKind::Match {
            value: Box::new(nil()),
            arms: vec![(HirPattern::Var(Binding(0)), None, nil())],
        },
        Span::synthetic(),
    );
    assert!(!m.allocates());
}

#[test]
fn match_with_array_rest_pattern_allocates() {
    let pat = HirPattern::Array {
        elements: vec![],
        rest: Some(Box::new(HirPattern::Var(Binding(0)))),
    };
    let m = Hir::silent(
        HirKind::Match {
            value: Box::new(nil()),
            arms: vec![(pat, None, nil())],
        },
        Span::synthetic(),
    );
    assert!(m.allocates());
}

#[test]
fn match_with_struct_rest_pattern_allocates() {
    let pat = HirPattern::Struct {
        entries: vec![(PatternKey::Keyword("a".to_string()), HirPattern::Wildcard)],
        rest: Some(Box::new(HirPattern::Var(Binding(0)))),
    };
    let m = Hir::silent(
        HirKind::Match {
            value: Box::new(nil()),
            arms: vec![(pat, None, nil())],
        },
        Span::synthetic(),
    );
    assert!(m.allocates());
}

#[test]
fn make_cell_does_not_allocate() {
    // MakeCell is transparent in the lowerer — the implicit
    // MakeCaptureCell happens at the binding site, not here.
    let m = Hir::silent(
        HirKind::MakeCell {
            value: Box::new(int(1)),
        },
        Span::synthetic(),
    );
    assert!(!m.allocates());
}

#[test]
fn deref_cell_does_not_allocate() {
    let d = Hir::silent(
        HirKind::DerefCell {
            cell: Box::new(Hir::silent(HirKind::Var(Binding(0)), Span::synthetic())),
        },
        Span::synthetic(),
    );
    assert!(!d.allocates());
}
