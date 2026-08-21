use super::*;

#[test]
fn test_syntax_construction() {
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Int(42), span.clone());
    assert_eq!(syntax.scopes.len(), 0);
    assert_eq!(syntax.span.start, 0);
    assert_eq!(syntax.span.end, 5);
}

#[test]
fn test_syntax_with_scopes() {
    let span = Span::new(0, 5, 1, 1);
    let scopes = vec![ScopeId(1), ScopeId(2)];
    let syntax = Syntax::with_scopes(SyntaxKind::Symbol("x".to_string()), span, scopes.clone());
    assert_eq!(syntax.scopes.len(), 2);
    assert_eq!(syntax.scopes[0], ScopeId(1));
    assert_eq!(syntax.scopes[1], ScopeId(2));
}

#[test]
fn test_add_scope() {
    let span = Span::new(0, 5, 1, 1);
    let mut syntax = Syntax::new(SyntaxKind::Symbol("x".to_string()), span);
    assert_eq!(syntax.scopes.len(), 0);

    syntax.add_scope(ScopeId(1));
    assert_eq!(syntax.scopes.len(), 1);
    assert_eq!(syntax.scopes[0], ScopeId(1));

    // Adding same scope again should not duplicate
    syntax.add_scope(ScopeId(1));
    assert_eq!(syntax.scopes.len(), 1);

    // Adding different scope should work
    syntax.add_scope(ScopeId(2));
    assert_eq!(syntax.scopes.len(), 2);
}

#[test]
fn test_is_symbol() {
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Symbol("foo".to_string()), span);
    assert!(syntax.is_symbol("foo"));
    assert!(!syntax.is_symbol("bar"));
}

#[test]
fn test_as_symbol() {
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Symbol("foo".to_string()), span);
    assert_eq!(syntax.as_symbol(), Some("foo"));
}

#[test]
fn test_as_list() {
    let span = Span::new(0, 5, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
        Syntax::new(SyntaxKind::Int(1), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::List(items.clone()), span);

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
    let true_syntax = Syntax::new(SyntaxKind::Bool(true), span.clone());
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
    for &v in &[7.0f64, 2.0, 1000.0, -3.0, 1.5, 0.5, 1e21] {
        let text = Syntax::new(SyntaxKind::Float(v), Span::synthetic()).to_string();
        let forms = crate::reader::read_syntax_all(&text, "<round-trip>").unwrap();
        assert_eq!(forms.len(), 1, "float {v} displayed as {text:?}");
        match &forms[0].kind {
            SyntaxKind::Float(f) => assert_eq!(*f, v, "value changed round-tripping {text:?}"),
            other => panic!("float {v} displayed as {text:?}, re-read as {other:?} (not a float)"),
        }
    }
}

#[test]
fn test_display_symbol() {
    let span = Span::new(0, 3, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Symbol("foo".to_string()), span);
    assert_eq!(syntax.to_string(), "foo");
}

#[test]
fn test_display_keyword() {
    let span = Span::new(0, 4, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Keyword("key".to_string()), span);
    assert_eq!(syntax.to_string(), ":key");
}

#[test]
fn test_display_string() {
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::String("hello".to_string()), span);
    assert_eq!(syntax.to_string(), "\"hello\"");
}

#[test]
fn test_display_list() {
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::List(items), span);
    assert_eq!(syntax.to_string(), "(a 1 2)");
}

#[test]
fn test_display_tuple() {
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::Array(items), span);
    assert_eq!(syntax.to_string(), "[1 2]");
}

#[test]
fn test_display_array() {
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::ArrayMut(items), span);
    assert_eq!(syntax.to_string(), "@[1 2]");
}

#[test]
fn test_display_quote() {
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let syntax = Syntax::new(SyntaxKind::Quote(Box::new(inner)), span);
    assert_eq!(syntax.to_string(), "'x");
}

#[test]
fn test_display_quasiquote() {
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let syntax = Syntax::new(SyntaxKind::Quasiquote(Box::new(inner)), span);
    assert_eq!(syntax.to_string(), "`x");
}

#[test]
fn test_display_unquote() {
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let syntax = Syntax::new(SyntaxKind::Unquote(Box::new(inner)), span);
    assert_eq!(syntax.to_string(), ",x");
}

#[test]
fn test_display_unquote_splicing() {
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let syntax = Syntax::new(SyntaxKind::UnquoteSplicing(Box::new(inner)), span);
    assert_eq!(syntax.to_string(), ",;x");
}

#[test]
fn test_expander_fresh_scope() {
    let mut expander = Expander::new();
    let scope1 = expander.fresh_scope();
    let scope2 = expander.fresh_scope();
    assert_ne!(scope1, scope2);
    assert_eq!(scope1, ScopeId(1));
    assert_eq!(scope2, ScopeId(2));
}

#[test]
fn test_expander_no_macros() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 5, 1, 1);
    let syntax = Syntax::new(SyntaxKind::Int(42), span);
    let result = expander.expand(syntax.clone(), &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "42");
}

#[test]
fn test_expander_expand_list() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("+".to_string()), span.clone()),
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::List(items), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "(+ 1 2)");
}

#[test]
fn test_expander_expand_tuple() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::Array(items), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "[1 2]");
}

#[test]
fn test_expander_expand_array() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 10, 1, 1);
    let items = vec![
        Syntax::new(SyntaxKind::Int(1), span.clone()),
        Syntax::new(SyntaxKind::Int(2), span.clone()),
    ];
    let syntax = Syntax::new(SyntaxKind::ArrayMut(items), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "@[1 2]");
}

#[test]
fn test_expander_quote_not_expanded() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 5, 1, 1);
    let inner = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let syntax = Syntax::new(SyntaxKind::Quote(Box::new(inner)), span);
    let result = expander.expand(syntax.clone(), &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), syntax.to_string());
}

#[test]
fn test_macro_definition_and_expansion() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let mut symbols = crate::symbol::SymbolTable::new();
        let mut vm = crate::vm::VM::new();
        let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
        // Seed `eval_meta` (primitive metadata) so compiling the macro's
        // transformer body resolves primitives — what `CompileCtx::new` does.
        // A bare `Expander::new()` starts with empty `eval_meta`.
        expander.set_eval_meta(crate::primitives::build_primitive_meta(&mut symbols));
        let span = Span::new(0, 5, 1, 1);

        // Define a simple macro: (defmacro double (x) `(+ ,x ,x))
        let template = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(
                SyntaxKind::List(vec![
                    Syntax::new(SyntaxKind::Symbol("+".to_string()), span.clone()),
                    Syntax::new(
                        SyntaxKind::Unquote(Box::new(Syntax::new(
                            SyntaxKind::Symbol("x".to_string()),
                            span.clone(),
                        ))),
                        span.clone(),
                    ),
                    Syntax::new(
                        SyntaxKind::Unquote(Box::new(Syntax::new(
                            SyntaxKind::Symbol("x".to_string()),
                            span.clone(),
                        ))),
                        span.clone(),
                    ),
                ]),
                span.clone(),
            ))),
            span.clone(),
        );

        let macro_def = MacroDef {
            name: "double".to_string(),
            params: vec!["x".to_string()],
            optional_params: vec![],
            rest_param: None,
            template,
            cached_transformer: std::rc::Rc::new(std::cell::RefCell::new(None)),
        };

        expander.define_macro(macro_def);

        // Expand (double 5)
        let call = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
                Syntax::new(SyntaxKind::Int(5), span.clone()),
            ]),
            span,
        );

        let result = expander.expand(call, &mut symbols, &mut vm);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // The result should be (+ 5 5)
        assert_eq!(expanded.to_string(), "(+ 5 5)");
    });
}

#[test]
fn test_macro_arity_check() {
    let mut expander = Expander::new();
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = crate::vm::VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    let span = Span::new(0, 5, 1, 1);

    let template = Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone());
    let macro_def = MacroDef {
        name: "single".to_string(),
        params: vec!["x".to_string()],
        optional_params: vec![],
        rest_param: None,
        template,
        cached_transformer: std::rc::Rc::new(std::cell::RefCell::new(None)),
    };

    expander.define_macro(macro_def);

    // Try to call with wrong arity
    let call = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("single".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(1), span.clone()),
            Syntax::new(SyntaxKind::Int(2), span.clone()),
        ]),
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
    assert_eq!(span.file, Some("test.el".to_string()));
    assert_eq!(span.to_string(), "test.el:1:1");
}

#[test]
fn test_span_synthetic() {
    let span = Span::synthetic();
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 0);
    assert_eq!(span.line, 0);
    assert_eq!(span.col, 0);
    assert_eq!(span.file, None);
}
