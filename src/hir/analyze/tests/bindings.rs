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
    // user binder x{} are distinct identities (docs/bindings.md "Duplicates
    // are judged by binding identity"): no duplicate error, and each
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
    // ((fn [] (def x 1) (def x 2) x)) — a fn body is a letrec* context
    // (docs/bindings.md "Function bodies are an implicit letrec"): the
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
    let HirKind::Lambda { body, .. } = &result.hir.kind else {
        panic!("expected Lambda, got {:?}", result.hir.kind);
    };
    let HirKind::Begin(exprs) = &body.kind else {
        panic!("expected Begin body, got {:?}", body.kind);
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
    // run (docs/bindings.md "Use before initialization is an error").
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
    // until after every initializer has run (docs/bindings.md).
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
fn letrec_self_reference_classifies_recursive() {
    use crate::hir::CaptureKind;

    // (letrec [loop (fn [m] (loop m))] (loop 3)) — `loop`'s initializer lambda
    // references the binding `loop` across the lambda boundary: a SELF-edge in the
    // enclosing SCC, classified `CaptureKind::Recursive` (carrying the SCC binding
    // identity), distinct from a sibling/foreign `Local`/`Capture`.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let lambda = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("m")]),
        make_list(vec![make_symbol("loop"), make_symbol("m")]),
    ]);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![make_symbol("loop"), lambda]),
        make_list(vec![make_symbol("loop"), make_int(3)]),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    let HirKind::Letrec { bindings, .. } = &result.hir.kind else {
        panic!("expected Letrec, got {:?}", result.hir.kind);
    };
    let (loop_b, loop_lambda) = &bindings[0];
    let HirKind::Lambda { captures, .. } = &loop_lambda.kind else {
        panic!("expected Lambda init, got {:?}", loop_lambda.kind);
    };
    let self_cap = captures
        .iter()
        .find(|c| c.binding == *loop_b)
        .expect("loop's initializer lambda must capture its own binding (the self-edge)");
    assert!(
        matches!(self_cap.kind, CaptureKind::Recursive { binding } if binding == *loop_b),
        "the same-binding self-reference must classify as CaptureKind::Recursive carrying \
         the SCC binding, got {:?}",
        self_cap.kind,
    );
}

#[test]
fn letrec_mutual_sibling_is_not_recursive() {
    use crate::hir::CaptureKind;

    // (letrec [ev (fn [m] (od m)) od (fn [m] (ev m))] (ev 3)) — the DISCRIMINATOR:
    // `ev`/`od` each capture the OTHER, never themselves, so every capture is a
    // sibling edge (`Local`/`Capture`), never `Recursive`. The self-vs-sibling split
    // is what partitions no-cell (self) from the closure-cycle merge (mutual).
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let ev_lambda = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("m")]),
        make_list(vec![make_symbol("od"), make_symbol("m")]),
    ]);
    let od_lambda = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("m")]),
        make_list(vec![make_symbol("ev"), make_symbol("m")]),
    ]);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![
            make_symbol("ev"),
            ev_lambda,
            make_symbol("od"),
            od_lambda,
        ]),
        make_list(vec![make_symbol("ev"), make_int(3)]),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();
    let HirKind::Letrec { bindings, .. } = &result.hir.kind else {
        panic!("expected Letrec, got {:?}", result.hir.kind);
    };
    let (ev_b, ev_lambda_hir) = &bindings[0];
    let (od_b, _) = &bindings[1];
    let HirKind::Lambda { captures, .. } = &ev_lambda_hir.kind else {
        panic!("expected Lambda init, got {:?}", ev_lambda_hir.kind);
    };
    assert!(
        captures.iter().any(|c| c.binding == *od_b),
        "ev's lambda must capture its sibling od",
    );
    for c in captures {
        assert!(
            !matches!(c.kind, CaptureKind::Recursive { .. }),
            "a purely-mutual member has only sibling captures, none Recursive: binding \
             {:?} captured as {:?}",
            c.binding,
            c.kind,
        );
    }
    let _ = ev_b;
}

#[test]
fn nested_lambda_reference_to_outer_binding_is_not_recursive() {
    use crate::hir::CaptureKind;

    // (letrec [loop (fn [m] (let [g (fn [] loop)] (g)))] (loop 3)) — inside `loop`'s
    // body a NESTED lambda `g` closes over `loop`. `loop` is `g`'s SIBLING capture,
    // never `g`'s self-edge: the self-edge is ONLY `loop`'s own initializer-lambda
    // reference to `loop`, one function level below the `letrec`. A reference from the
    // deeper `g` must classify `Local`/`Capture`, never `Recursive` — else the lowerer
    // would materialize `g` for it (LoadSelf names the executing closure), not `loop`.
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let g_lambda = make_list(vec![
        make_symbol("fn"),
        make_list(vec![]),
        make_symbol("loop"),
    ]);
    let inner_let = make_list(vec![
        make_symbol("let"),
        make_array(vec![make_symbol("g"), g_lambda]),
        make_list(vec![make_symbol("g")]),
    ]);
    let loop_lambda = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("m")]),
        inner_let,
    ]);
    let syntax = make_list(vec![
        make_symbol("letrec"),
        make_array(vec![make_symbol("loop"), loop_lambda]),
        make_list(vec![make_symbol("loop"), make_int(3)]),
    ]);
    let result = analyzer.analyze(&syntax).unwrap();

    // Walk to the innermost (zero-param) lambda `g` and read its captures.
    fn find_g_captures(h: &Hir) -> Option<Vec<crate::hir::CaptureInfo>> {
        if let HirKind::Lambda {
            params, captures, ..
        } = &h.kind
        {
            if params.is_empty() {
                return Some(captures.clone());
            }
        }
        let mut found = None;
        h.for_each_child(|c| {
            if found.is_none() {
                found = find_g_captures(c);
            }
        });
        found
    }
    let g_captures = find_g_captures(&result.hir).expect("the nested zero-param lambda g exists");
    let loop_cap = g_captures
        .iter()
        .find(|c| {
            // `loop`'s binding is bindings[0].0; locate it via the Letrec.
            matches!(&result.hir.kind, HirKind::Letrec { bindings, .. } if c.binding == bindings[0].0)
        })
        .expect("g must capture the enclosing loop binding");
    assert!(
        !matches!(loop_cap.kind, CaptureKind::Recursive { .. }),
        "a nested lambda's reference to the enclosing self-recursive binding is a \
         sibling capture, not a self-edge: got {:?}",
        loop_cap.kind,
    );
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
