// audited: 2026-11-09
//! The rules a `letrec` and a function body hold their binders to: duplicates, redefinition, mutability, and use before initialization.
//!
//! docs/bindings.md

use super::*;

#[test]
fn letrec_duplicate_same_identity_errors() {
    // (letrec [x 1 x 2] x) — same name, same (empty) scope set: rejected.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![
            make_symbol("x"),
            make_int(1),
            make_symbol("x"),
            make_int(2),
        ]),
        make_symbol("x"),
    ]);
    let Err(err) = analyzer.analyze(&syntax) else {
        panic!("expected a duplicate-binding error");
    };
    assert!(err.contains("duplicate binding 'x'"), "got: {err}");
}

#[test]
fn letrec_hygiene_distinct_scopes_resolve_to_their_own() {
    // (letrec [x{3} 1  x{} 2] <body>) — a macro-template binder x{3} and a
    // user binder x{} are distinct identities: no duplicate error, and each
    // reference resolves to its own binder by scope-subset matching.
    let make = |body: Syntax| {
        make_list(vec![
            make_symbol("letrec"),
            make_array(vec![
                make_symbol_scoped("x", &[3]),
                make_int(1),
                make_symbol("x"),
                make_int(2),
            ]),
            body,
        ])
    };

    // A user-scoped reference x{} sees only the user binder.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let result = analyzer.analyze(&make(make_symbol("x"))).unwrap();
    let HirKind::Letrec { bindings, body } = &result.hir.kind else {
        panic!("expected Letrec, got {:?}", result.hir.kind);
    };
    assert_eq!(bindings.len(), 2, "both binders must survive");
    let HirKind::Var(b) = unwrap_single(body).kind else {
        panic!("expected Var body, got {:?}", body.kind);
    };
    assert_eq!(b, bindings[1].0, "user ref must see the user binder");

    // A template-scoped reference x{3} sees the template binder
    // (both match; the larger scope set wins).
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let result = analyzer
        .analyze(&make(make_symbol_scoped("x", &[3])))
        .unwrap();
    let HirKind::Letrec { bindings, body } = &result.hir.kind else {
        panic!("expected Letrec, got {:?}", result.hir.kind);
    };
    let HirKind::Var(b) = unwrap_single(body).kind else {
        panic!("expected Var body, got {:?}", body.kind);
    };
    assert_eq!(
        b, bindings[0].0,
        "template ref must see the template binder"
    );
}

// Counter-factual: a function body rejected the second def as a duplicate.
#[test]
fn fn_body_redefinition_is_a_new_binding() {
    // (fn [] (def x 1) (def x 2) x) — each def makes its own binding, and the
    // read after the second resolves to it.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_list(vec![make_symbol("def"), make_symbol("x"), make_int(1)]),
        make_list(vec![make_symbol("def"), make_symbol("x"), make_int(2)]),
        make_symbol("x"),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    let HirKind::Lambda(lambda) = &result.hir.kind else {
        panic!("expected Lambda, got {:?}", result.hir.kind);
    };
    let HirKind::Begin(exprs) = &lambda.body.kind else {
        panic!("expected Begin body, got {:?}", lambda.body.kind);
    };
    let [HirKind::Define { binding: first, .. }, HirKind::Define {
        binding: second, ..
    }, HirKind::Var(read)] = [&exprs[0].kind, &exprs[1].kind, &exprs[2].kind]
    else {
        panic!("expected two Defines and a Var, got {:?}", exprs);
    };
    assert_ne!(first, second, "the second def makes a new binding");
    assert_eq!(read, second, "the read after it resolves to the second def");
}

#[test]
fn fn_body_mixed_mutability_errors() {
    // (fn [] (def x 1) (def @x 2) x) — every def of a name in one scope
    // agrees on mutability.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_list(vec![make_symbol("def"), make_symbol("x"), make_int(1)]),
        make_list(vec![make_symbol("def"), make_symbol("@x"), make_int(2)]),
        make_symbol("x"),
    ]);
    let Err(err) = analyzer.analyze(&syntax) else {
        panic!("expected a mutability error");
    };
    assert!(err.contains("both mutable and immutable"), "got: {err}");
}

#[test]
fn fn_body_hygiene_distinct_scopes_allowed() {
    // (fn [] (def x{3} 1) (def x{} 2) x{}) — macro-introduced define and
    // user define are distinct identities: no error, and the user
    // reference resolves to the user define.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_list(vec![
            make_symbol("def"),
            make_symbol_scoped("x", &[3]),
            make_int(1),
        ]),
        make_list(vec![make_symbol("def"), make_symbol("x"), make_int(2)]),
        make_symbol("x"),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    let HirKind::Lambda(lambda) = &result.hir.kind else {
        panic!("expected Lambda, got {:?}", result.hir.kind);
    };
    let HirKind::Begin(exprs) = &lambda.body.kind else {
        panic!("expected Begin body, got {:?}", lambda.body.kind);
    };
    let HirKind::Define {
        binding: user_def, ..
    } = exprs[1].kind
    else {
        panic!("expected Define, got {:?}", exprs[1].kind);
    };
    let HirKind::Var(b) = exprs[2].kind else {
        panic!("expected Var, got {:?}", exprs[2].kind);
    };
    assert_eq!(b, user_def, "user ref must see the user define");
}

#[test]
fn letrec_use_before_init_errors() {
    // (letrec [a b b 7] a) — b's value is read before its initializer has
    // run.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![
            make_symbol("a"),
            make_symbol("b"),
            make_symbol("b"),
            make_int(7),
        ]),
        make_symbol("a"),
    ]);
    let Err(err) = analyzer.analyze(&syntax) else {
        panic!("expected a use-before-init error");
    };
    assert!(
        err.contains("'b' referenced before its initialization"),
        "got: {err}"
    );
}

#[test]
fn letrec_backward_dep_ok() {
    // (letrec [a 1 b a] b) — a is initialized when b's initializer runs.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![
            make_symbol("a"),
            make_int(1),
            make_symbol("b"),
            make_symbol("a"),
        ]),
        make_symbol("b"),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::Letrec { .. }));
}

#[test]
fn letrec_forward_ref_through_lambda_ok() {
    // (letrec [a (fn [] b) b 7] (a)) — the lambda defers the use of b
    // until after every initializer has run.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![
            make_symbol("a"),
            make_list(vec![make_symbol("fn"), make_list(vec![]), make_symbol("b")]),
            make_symbol("b"),
            make_int(7),
        ]),
        make_list(vec![make_symbol("a")]),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::Letrec { .. }));
}

#[test]
fn fn_body_use_before_init_errors() {
    // ((fn [] (def a b) (def b 7) a)) — no binding of b exists outside the
    // body, so reading b's value before its initializer runs is an error.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_list(vec![make_symbol("def"), make_symbol("a"), make_symbol("b")]),
        make_list(vec![make_symbol("def"), make_symbol("b"), make_int(7)]),
        make_symbol("a"),
    ]);
    let Err(err) = analyzer.analyze(&syntax) else {
        panic!("expected a use-before-init error");
    };
    assert!(
        err.contains("'b' referenced before its initialization"),
        "got: {err}"
    );
}

// Counter-factual: the body's pending b hid the parameter b, so the read was
// a use-before-init error.
#[test]
fn fn_body_read_before_def_finds_the_parameter() {
    // (fn [b] (def a b) (def b 7) a) — the read in a's value comes before the
    // body's def of b, so it resolves to the parameter.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("b")]),
        make_list(vec![make_symbol("def"), make_symbol("a"), make_symbol("b")]),
        make_list(vec![make_symbol("def"), make_symbol("b"), make_int(7)]),
        make_symbol("a"),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    let HirKind::Lambda(lambda) = &result.hir.kind else {
        panic!("expected Lambda, got {:?}", result.hir.kind);
    };
    let HirKind::Begin(exprs) = &lambda.body.kind else {
        panic!("expected Begin body, got {:?}", lambda.body.kind);
    };
    let HirKind::Define { value, .. } = &exprs[0].kind else {
        panic!("expected Define, got {:?}", exprs[0].kind);
    };
    assert!(
        matches!(value.kind, HirKind::Var(b) if b == lambda.params[0]),
        "a's value reads the parameter, got {:?}",
        value.kind
    );
}
