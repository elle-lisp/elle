// audited: 2026-10-30
//! The `letrec*` rules the analyzer enforces: duplicates by binding identity, and use before initialization.
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

#[test]
fn fn_body_duplicate_def_errors() {
    // ((fn [] (def x 1) (def x 2) x)) — a fn body is a letrec* context: the
    // same-identity duplicate is rejected like an explicit letrec's.
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
    let Err(err) = analyzer.analyze(&syntax) else {
        panic!("expected a duplicate-binding error");
    };
    assert!(err.contains("duplicate binding 'x'"), "got: {err}");
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
    // ((fn [] (def a b) (def b 7) a)) — a fn body is a letrec* context;
    // reading b's value before its initializer runs is an error.
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
