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
fn lambda_allocates() {
    let lam = Hir::silent(
        HirKind::Lambda {
            params: vec![],
            num_required: 0,
            rest_param: None,
            vararg_kind: VarargKind::List,
            captures: vec![],
            body: Box::new(nil()),
            num_locals: 0,
            inferred_signals: crate::signals::Signal::silent(),
            param_bounds: vec![],
            doc: None,
            origin: None,
            assert_numeric: false,
        },
        Span::synthetic(),
    );
    assert!(lam.allocates());
}

#[test]
fn allocating_intrinsic_propagates() {
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
    // The If form is propagating; its allocations come from branches,
    // which the ANF traversal handles separately. The If node itself
    // does not produce a fresh heap value at its own HirId.
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
