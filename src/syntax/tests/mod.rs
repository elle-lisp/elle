use super::*;

mod region;

/// A heap and arena for a test that builds nodes by hand. Keep the returned
/// `SyntaxHeap` alive for as long as the nodes.
fn arena() -> (SyntaxHeap, SyntaxArena) {
    SyntaxHeap::with_arena()
}

/// An expander over `vm`'s heap, with a fresh working arena.
fn expander_on(vm: &mut crate::vm::VM) -> (Expander, SyntaxArena) {
    let arena = SyntaxArena::mint(vm.heap());
    let mut e = Expander::on_vm(vm);
    e.set_arena(arena);
    (e, arena)
}

#[test]
fn test_syntax_construction() {
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Int(42), span);
    assert_eq!(syntax.scopes().len(), 0);
    assert_eq!(syntax.span.start, 0);
    assert_eq!(syntax.span.end, 5);
}

#[test]
fn test_syntax_with_scopes() {
    let (_home, a) = arena();
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::symbol_scoped(&a, "x", span, &[ScopeId(1), ScopeId(2)]);
    assert_eq!(syntax.scopes().len(), 2);
    assert_eq!(syntax.scopes()[0], ScopeId(1));
    assert_eq!(syntax.scopes()[1], ScopeId(2));
}

#[test]
fn test_add_scope() {
    let (_home, a) = arena();
    let span = Span::new(0, 5, 1, 1);
    let mut syntax = Syntax::symbol(&a, "x", span);
    assert_eq!(syntax.scopes().len(), 0);

    syntax.add_scope(&a, ScopeId(1));
    assert_eq!(syntax.scopes().len(), 1);
    assert_eq!(syntax.scopes()[0], ScopeId(1));

    // Adding same scope again should not duplicate
    syntax.add_scope(&a, ScopeId(1));
    assert_eq!(syntax.scopes().len(), 1);

    // Adding different scope should work
    syntax.add_scope(&a, ScopeId(2));
    assert_eq!(syntax.scopes().len(), 2);
}

#[test]
fn test_is_symbol() {
    let (_home, a) = arena();
    let syntax = Syntax::symbol(&a, "foo", Span::new(0, 5, 1, 1));
    assert!(syntax.is_symbol("foo"));
    assert!(!syntax.is_symbol("bar"));
}

#[test]
fn test_as_symbol() {
    let (_home, a) = arena();
    let syntax = Syntax::symbol(&a, "foo", Span::new(0, 5, 1, 1));
    assert_eq!(syntax.as_symbol(), Some("foo"));
}

#[test]
fn test_as_list() {
    let (_home, a) = arena();
    let span = Span::new(0, 5, 1, 1);
    let items = [
        Syntax::symbol(&a, "a", span),
        Syntax::new(SyntaxKind::Int(1), span),
    ];
    let syntax = Syntax::list(&a, &items, span);

    let list = syntax.as_list();
    assert!(list.is_some());
    assert_eq!(list.unwrap().len(), 2);
}

#[test]
fn test_display_nil() {
    let span = Span::new(0, 3, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Nil, span);
    assert_eq!(syntax.to_string(), "nil");
}

#[test]
fn test_display_bool() {
    let span = Span::new(0, 2, 1, 1);
    let true_syntax = Syntax::new(SyntaxKind::Bool(true), span);
    let false_syntax = Syntax::new(SyntaxKind::Bool(false), span);
    assert_eq!(true_syntax.to_string(), "true");
    assert_eq!(false_syntax.to_string(), "false");
}

#[test]
fn test_display_int() {
    let span = Span::new(0, 2, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Int(42), span);
    assert_eq!(syntax.to_string(), "42");
}

#[test]
fn test_display_float() {
    let span = Span::new(0, 3, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Float(std::f64::consts::PI), span);
    assert_eq!(syntax.to_string(), std::f64::consts::PI.to_string());
}

#[test]
fn test_display_float_integral_round_trips() {
    // `Syntax` Display must round-trip through the reader for EVERY literal —
    // most subtly an integral-valued float. `splice_includes` (the WASM
    // full-module path) re-stringifies each user form through this Display and
    // re-reads it, so a `Float(7.0)` rendered as the bare integer `7` re-reads
    // as `Int(7)` — silently changing `(type-of 7.0)` from :float to :integer.
    // Non-integral floats (1.5) never hit this because their text already
    // carries a '.'; that is why the bug hid behind `test_display_float`'s PI.
    let (_home, a) = arena();
    for &v in &[7.0f64, 2.0, 1000.0, -3.0, 1.5, 0.5, 1e21] {
        let text = Syntax::new(SyntaxKind::Float(v), Span::synthetic()).to_string();
        let forms = crate::reader::read_syntax_all(a, &text, "<round-trip>").unwrap();
        assert_eq!(forms.len(), 1, "float {v} displayed as {text:?}");
        match &forms[0].kind {
            SyntaxKind::Float(f) => assert_eq!(*f, v, "value changed round-tripping {text:?}"),
            other => panic!("float {v} displayed as {text:?}, re-read as {other:?} (not a float)"),
        }
    }
}

#[test]
fn test_display_symbol() {
    let (_home, a) = arena();
    let syntax = Syntax::symbol(&a, "foo", Span::new(0, 3, 1, 1));
    assert_eq!(syntax.to_string(), "foo");
}

#[test]
fn test_display_keyword() {
    let (_home, a) = arena();
    let syntax = Syntax::keyword(&a, "key", Span::new(0, 4, 1, 1));
    assert_eq!(syntax.to_string(), ":key");
}

#[test]
fn test_display_string() {
    let (_home, a) = arena();
    let syntax = Syntax::string(&a, "hello", Span::new(0, 5, 1, 1));
    assert_eq!(syntax.to_string(), "\"hello\"");
}

#[test]
fn test_display_list() {
    let (_home, a) = arena();
    let span = Span::new(0, 10, 1, 1);
    let items = [
        Syntax::symbol(&a, "a", span),
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];
    let syntax = Syntax::list(&a, &items, span);
    assert_eq!(syntax.to_string(), "(a 1 2)");
}

#[test]
fn test_display_tuple() {
    let (_home, a) = arena();
    let span = Span::new(0, 10, 1, 1);
    let items = [
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];
    let syntax = Syntax::array(&a, &items, span);
    assert_eq!(syntax.to_string(), "[1 2]");
}

#[test]
fn test_display_array() {
    let (_home, a) = arena();
    let span = Span::new(0, 10, 1, 1);
    let items = [
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];
    let syntax = Syntax::new(SyntaxKind::ArrayMut(a.nodes(&items)), span);
    assert_eq!(syntax.to_string(), "@[1 2]");
}

/// Every wrapping kind prints its reader shorthand. One test per kind was one
/// assertion each; the table keeps the constructor and the spelling side by
/// side, which is the pair that has to agree.
#[test]
fn test_display_wrapping_kinds() {
    let (_home, a) = arena();
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::symbol(&a, "x", span);
    let cases: [(WrapCtor, &str); 5] = [
        (SyntaxKind::Quote, "'x"),
        (SyntaxKind::Quasiquote, "`x"),
        (SyntaxKind::Unquote, ",x"),
        (SyntaxKind::UnquoteSplicing, ",;x"),
        (SyntaxKind::Splice, ";x"),
    ];
    for (make, expected) in cases {
        let syntax = Syntax::new(make(a.node(inner)), span);
        assert_eq!(syntax.to_string(), expected);
    }
}

#[test]
fn test_expander_fresh_scope() {
    let mut vm = crate::vm::VM::new();
    let (mut expander, _a) = expander_on(&mut vm);
    let scope1 = expander.fresh_scope();
    let scope2 = expander.fresh_scope();
    assert_ne!(scope1, scope2);
    assert_eq!(scope1, ScopeId(1));
    assert_eq!(scope2, ScopeId(2));
}

#[test]
fn test_expander_no_macros() {
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let (mut expander, _a) = expander_on(&mut vm);
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Int(42), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "42");
}

/// Sequence literals pass through expansion with their kind intact.
#[test]
fn test_expander_expands_sequence_literals() {
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let (mut expander, a) = expander_on(&mut vm);
    let span = Span::new(0, 10, 1, 1);
    let ints = [
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];
    let call = [
        Syntax::symbol(&a, "+", span),
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];

    let cases: [(SyntaxKind, &str); 3] = [
        (SyntaxKind::List(a.nodes(&call)), "(+ 1 2)"),
        (SyntaxKind::Array(a.nodes(&ints)), "[1 2]"),
        (SyntaxKind::ArrayMut(a.nodes(&ints)), "@[1 2]"),
    ];
    for (kind, expected) in cases {
        let expanded = expander
            .expand(Syntax::new(kind, span), &mut symbols, &mut vm)
            .expect("expansion succeeds");
        assert_eq!(expanded.to_string(), expected);
    }
}

#[test]
fn test_expander_quote_not_expanded() {
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let (mut expander, a) = expander_on(&mut vm);
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::quote(&a, Syntax::symbol(&a, "x", span), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), syntax.to_string());
}

/// Build a `MacroDef` with no optional or rest parameters.
fn macro_def(name: &str, params: &[&str], template: Syntax) -> MacroDef {
    MacroDef {
        name: name.to_string(),
        params: params.iter().map(|p| p.to_string()).collect(),
        optional_params: vec![],
        rest_param: None,
        template,
        cached_transformer: std::rc::Rc::new(std::cell::RefCell::new(None)),
    }
}

#[test]
fn test_macro_definition_and_expansion() {
    crate::value::arena::with_test_region(|| {
        let mut symbols = crate::symbol::SymbolTable::new();
        let mut vm = crate::vm::VM::new();
        let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
        let (mut expander, a) = expander_on(&mut vm);
        // Seed `eval_meta` (primitive metadata) so compiling the macro's
        // transformer body resolves primitives — what `CompileCtx::new` does.
        // A bare `Expander::new()` starts with empty `eval_meta`.
        expander.set_eval_meta(crate::primitives::build_primitive_meta(&mut symbols));
        let span = Span::new(0, 5, 1, 1);

        // Define a simple macro: (defmacro double (x) `(+ ,x ,x))
        let unquoted_x = Syntax::new(
            SyntaxKind::Unquote(a.node(Syntax::symbol(&a, "x", span))),
            span,
        );
        let body = Syntax::list(
            &a,
            &[Syntax::symbol(&a, "+", span), unquoted_x, unquoted_x],
            span,
        );
        let template = Syntax::new(SyntaxKind::Quasiquote(a.node(body)), span);

        expander.define_macro(macro_def("double", &["x"], template));

        // Expand (double 5)
        let call = Syntax::list(
            &a,
            &[
                Syntax::symbol(&a, "double", span),
                Syntax::new(SyntaxKind::Int(5), span),
            ],
            span,
        );

        let result = expander.expand(call, &mut symbols, &mut vm);
        assert!(result.is_ok());
        // The result should be (+ 5 5)
        assert_eq!(result.unwrap().to_string(), "(+ 5 5)");
    });
}

#[test]
fn test_macro_arity_check() {
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let (mut expander, a) = expander_on(&mut vm);
    let span = Span::new(0, 5, 1, 1);

    let template = Syntax::symbol(&a, "x", span);
    expander.define_macro(macro_def("single", &["x"], template));

    // Try to call with wrong arity
    let call = Syntax::list(
        &a,
        &[
            Syntax::symbol(&a, "single", span),
            Syntax::new(SyntaxKind::Int(1), span),
            Syntax::new(SyntaxKind::Int(2), span),
        ],
        span,
    );

    let result = expander.expand(call, &mut symbols, &mut vm);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("expects 1 arguments, got 2"));
}

#[test]
fn test_scope_merge() {
    let span1 = Span::new(0, 5, 1, 1);
    let span2 = Span::new(10, 15, 2, 5);
    let merged = span1.merge(&span2);

    assert_eq!(merged.start, 0);
    assert_eq!(merged.end, 15);
    assert_eq!(merged.line, 1);
}

#[test]
fn test_span_with_file() {
    let span = Span::new(0, 5, 1, 1).with_file("test.el");
    assert_eq!(span.file(), Some("test.el"));
    assert_eq!(span.to_string(), "test.el:1:1");
}

#[test]
fn test_span_synthetic() {
    let span = Span::synthetic();
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 0);
    assert_eq!(span.line, 0);
    assert_eq!(span.col, 0);
    assert_eq!(span.file(), None);
}
