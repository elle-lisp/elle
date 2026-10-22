// audited: 2026-10-21
//! Which `assign` makes a closure a writer of the binding it names.
//!
//! docs/impl/graft/operands.md

use super::*;

/// Analyze a zero-parameter lambda whose body is `(var i 0)`, the given middle
/// form, `(assign i 2)` and `i`, and answer the binding of `i`.
fn counter_binding_with(middle: Syntax) -> (Binding, BindingArena) {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let result = {
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        let syntax = make_list(vec![
            make_symbol("fn"),
            make_list(vec![]),
            make_list(vec![make_symbol("var"), make_symbol("i"), make_int(0)]),
            middle,
            make_list(vec![make_symbol("assign"), make_symbol("i"), make_int(2)]),
            make_symbol("i"),
        ]);
        analyzer.analyze(&syntax).unwrap()
    };
    let HirKind::Lambda(lambda) = &result.hir.kind else {
        panic!("expected Lambda, got {:?}", result.hir.kind);
    };
    let HirKind::Begin(exprs) = &lambda.body.kind else {
        panic!("expected Begin body, got {:?}", lambda.body.kind);
    };
    let HirKind::Define { binding, .. } = exprs[0].kind else {
        panic!("expected Define, got {:?}", exprs[0].kind);
    };
    (binding, arena)
}

/// `(fn [] (var i 0) (fn [] (assign i 1)) (assign i 2) i)` — the nested lambda
/// assigns `i`, so a closure may write it. The counter-factual is a rule that
/// read only the lambda's own `assign` and answered no.
#[test]
fn an_assign_inside_a_nested_lambda_marks_the_closure_as_writer() {
    let writer = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_list(vec![make_symbol("assign"), make_symbol("i"), make_int(1)]),
    ]);
    let (i, arena) = counter_binding_with(writer);
    let inner = arena.get(i);
    assert!(inner.is_mutated, "i is assigned");
    assert!(
        inner.closure_may_write(),
        "the nested lambda assigns i, so a closure may write it"
    );
}

/// `(fn [] (var i 0) (fn [] i) (assign i 2) i)` — the nested lambda only reads
/// `i`, and the one `assign` sits in `i`'s own function, so no closure can
/// write it. The counter-factual is the capture standing in for a write, which
/// answered yes here and cost the graft a `let` per use of such a counter.
#[test]
fn an_assign_in_the_bindings_own_function_leaves_no_closure_as_writer() {
    let reader = make_list(vec![make_symbol("fn"), make_list(vec![]), make_symbol("i")]);
    let (i, arena) = counter_binding_with(reader);
    let inner = arena.get(i);
    assert!(inner.is_mutated, "i is assigned");
    assert!(
        inner.needs_capture(),
        "the reader captures i, so it has a cell"
    );
    assert!(
        !inner.closure_may_write(),
        "no lambda assigns i, so no closure can write it"
    );
}
