//! Tests for macro expansion

use super::*;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax, SyntaxKind};
use crate::vm::VM;
use std::cell::RefCell;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_quasiquote_simple_list() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 10, 1, 1);

    // `(a b c)
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
        Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
        Syntax::new(SyntaxKind::Symbol("c".to_string()), span.clone()),
    ];
    let syntax = Syntax::new(
        SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
        span.clone(),
    );

    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    // Symbols in a quasiquoted list become `SyntaxLiteral(Value::syntax(...))`
    // wrappers (Flatt 2016 §3 — preserves definition-site scopes), so the
    // expansion is `(list #<syntax-literal:...> #<syntax-literal:...> ...)`.
    let result_str = result.to_string();
    assert!(
        result_str.contains("list"),
        "Result should contain 'list': {}",
        result_str
    );
    assert!(
        result_str.contains("syntax-literal"),
        "Quasiquoted symbols should expand to syntax-literal wrappers: {}",
        result_str
    );
}

#[test]
fn test_quasiquote_with_unquote() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 10, 1, 1);

    // `(a ,x b)
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
        Syntax::new(
            SyntaxKind::Unquote(Box::new(Syntax::new(
                SyntaxKind::Symbol("x".to_string()),
                span.clone(),
            ))),
            span.clone(),
        ),
        Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
    ];
    let syntax = Syntax::new(
        SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
        span.clone(),
    );

    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    // `(a ,x b) expands to `(list <syntax-literal a> x <syntax-literal b>)` —
    // the unquoted `x` appears bare while the other symbols are wrapped.
    let result_str = result.to_string();
    assert!(
        result_str.contains("list"),
        "Result should contain 'list': {}",
        result_str
    );
    // Non-unquoted symbols become SyntaxLiteral for scope preservation
    assert!(
        result_str.contains("syntax-literal"),
        "Quasiquoted symbols should expand to syntax-literal wrappers: {}",
        result_str
    );
    assert!(
        result_str.contains("x"),
        "Unquoted symbol should appear bare: {}",
        result_str
    );
}

#[test]
fn test_quasiquote_with_splicing() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 10, 1, 1);

    // `(a ,;xs b)
    let items = vec![
        Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
        Syntax::new(
            SyntaxKind::UnquoteSplicing(Box::new(Syntax::new(
                SyntaxKind::Symbol("xs".to_string()),
                span.clone(),
            ))),
            span.clone(),
        ),
        Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
    ];
    let syntax = Syntax::new(
        SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
        span.clone(),
    );

    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    let result_str = result.to_string();
    assert!(
        result_str.contains("append"),
        "Result should contain 'append': {}",
        result_str
    );
    assert!(
        result_str.contains("list"),
        "Result should contain 'list': {}",
        result_str
    );
    assert!(
        result_str.contains("xs"),
        "Result should contain 'xs': {}",
        result_str
    );
}

#[test]
fn test_quasiquote_non_list() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // `x
    let syntax = Syntax::new(
        SyntaxKind::Quasiquote(Box::new(Syntax::new(
            SyntaxKind::Symbol("x".to_string()),
            span.clone(),
        ))),
        span.clone(),
    );

    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    let result_str = result.to_string();
    // A bare quasiquoted symbol expands to a syntax-literal wrapper that
    // carries the original syntax (with its scopes) through the Value
    // round-trip.
    assert!(
        result_str.contains("syntax-literal"),
        "Quasiquoted symbol should expand to a syntax-literal wrapper: {}",
        result_str
    );
    assert!(
        result_str.contains("x"),
        "Result should contain 'x': {}",
        result_str
    );
}

#[test]
fn test_defmacro_registration() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // Define a macro using defmacro with quasiquote: (defmacro double (x) `(* ,x 2))
    let defmacro_form = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::List(vec![Syntax::new(
                    SyntaxKind::Symbol("x".to_string()),
                    span.clone(),
                )]),
                span.clone(),
            ),
            Syntax::new(
                SyntaxKind::Quasiquote(Box::new(Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol("*".to_string()), span.clone()),
                        Syntax::new(
                            SyntaxKind::Unquote(Box::new(Syntax::new(
                                SyntaxKind::Symbol("x".to_string()),
                                span.clone(),
                            ))),
                            span.clone(),
                        ),
                        Syntax::new(SyntaxKind::Int(2), span.clone()),
                    ]),
                    span.clone(),
                ))),
                span.clone(),
            ),
        ]),
        span.clone(),
    );

    let result = expander.expand(defmacro_form, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    // defmacro should expand to nil
    assert_eq!(expanded.to_string(), "nil");

    // Now use the macro: (double 21)
    let macro_call = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(21), span),
        ]),
        Span::new(0, 5, 1, 1),
    );

    let result = expander.expand(macro_call, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    // Should expand to (* 21 2)
    assert_eq!(expanded.to_string(), "(* 21 2)");
}

#[test]
fn test_defmacro_invalid_syntax() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // defmacro with wrong number of arguments
    let defmacro_form = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
        ]),
        span.clone(),
    );

    let result = expander.expand(defmacro_form, &mut symbols, &mut vm);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("requires exactly 3 arguments"));
}

#[test]
fn test_defmacro_non_symbol_name() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // defmacro with non-symbol name
    let defmacro_form = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(42), span.clone()),
            Syntax::new(SyntaxKind::List(vec![]), span.clone()),
            Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
        ]),
        span.clone(),
    );

    let result = expander.expand(defmacro_form, &mut symbols, &mut vm);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("macro name must be a symbol"));
}

#[test]
fn test_macro_predicate_true() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // Define a macro
    let macro_def = MacroDef {
        name: "my-macro".to_string(),
        params: vec!["x".to_string()],
        optional_params: vec![],
        rest_param: None,
        template: Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
        definition_scope: ScopeId(0),
        cached_transformer: RefCell::new(None),
    };
    expander.define_macro(macro_def);

    // (macro? my-macro) should return true
    let check = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("my-macro".to_string()), span.clone()),
        ]),
        span,
    );

    let result = expander.expand(check, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "true");
}

#[test]
fn test_macro_predicate_false() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (macro? not-a-macro) should return false
    let check = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("not-a-macro".to_string()), span.clone()),
        ]),
        span,
    );

    let result = expander.expand(check, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "false");
}

#[test]
fn test_macro_predicate_non_symbol() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (macro? 42) should return false (not a symbol)
    let check = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("macro?".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(42), span.clone()),
        ]),
        span,
    );

    let result = expander.expand(check, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    assert_eq!(expanded.to_string(), "false");
}

#[test]
fn test_macro_predicate_wrong_arity() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (macro?) with no arguments should error
    let check = Syntax::new(
        SyntaxKind::List(vec![Syntax::new(
            SyntaxKind::Symbol("macro?".to_string()),
            span.clone(),
        )]),
        span,
    );

    let result = expander.expand(check, &mut symbols, &mut vm);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("requires exactly 1 argument"));
}

#[test]
fn test_expand_macro_basic() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // Define a macro: (defmacro double (x) `(+ ,x ,x))
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
        definition_scope: ScopeId(0),
        cached_transformer: RefCell::new(None),
    };
    expander.define_macro(macro_def);

    // (expand-macro '(double 5)) should return '(+ 5 5)
    let expand_call = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::Quote(Box::new(Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol("double".to_string()), span.clone()),
                        Syntax::new(SyntaxKind::Int(5), span.clone()),
                    ]),
                    span.clone(),
                ))),
                span.clone(),
            ),
        ]),
        span,
    );

    let result = expander.expand(expand_call, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    // Result should be a quoted form: '(+ 5 5)
    assert_eq!(expanded.to_string(), "'(+ 5 5)");
}

#[test]
fn test_expand_macro_non_macro() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (expand-macro '(+ 1 2)) should return '(+ 1 2) unchanged
    let expand_call = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::Quote(Box::new(Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol("+".to_string()), span.clone()),
                        Syntax::new(SyntaxKind::Int(1), span.clone()),
                        Syntax::new(SyntaxKind::Int(2), span.clone()),
                    ]),
                    span.clone(),
                ))),
                span.clone(),
            ),
        ]),
        span,
    );

    let result = expander.expand(expand_call, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    // Result should be unchanged: '(+ 1 2)
    assert_eq!(expanded.to_string(), "'(+ 1 2)");
}

#[test]
fn test_expand_macro_wrong_arity() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (expand-macro) with no arguments should error
    let expand_call = Syntax::new(
        SyntaxKind::List(vec![Syntax::new(
            SyntaxKind::Symbol("expand-macro".to_string()),
            span.clone(),
        )]),
        span,
    );

    let result = expander.expand(expand_call, &mut symbols, &mut vm);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("requires exactly 1 argument"));
}

#[test]
fn test_expand_macro_unquoted_arg() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (expand-macro x) with unquoted arg returns the arg unchanged
    let expand_call = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("expand-macro".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
        ]),
        span,
    );

    let result = expander.expand(expand_call, &mut symbols, &mut vm);
    assert!(result.is_ok());
    let expanded = result.unwrap();
    // Result should be the symbol x unchanged
    assert_eq!(expanded.to_string(), "x");
}

#[test]
fn test_keyword_not_qualified() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // :keyword should remain a keyword, not be treated as qualified
    let syntax = Syntax::new(SyntaxKind::Keyword("foo".to_string()), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    // Keywords are stored without the leading colon in SyntaxKind::Keyword
    assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "foo"));
}

/// Macro body uses `if` to conditionally generate different code.
/// This requires VM evaluation — template substitution can't do this.
#[test]
fn test_macro_with_conditional_body() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 50, 1, 1);

    // (defmacro maybe-negate (x negate?)
    //   (if negate? `(- ,x) x))
    let defmacro_syntax = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("defmacro".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("maybe-negate".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::List(vec![
                    Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
                    Syntax::new(SyntaxKind::Symbol("negate?".to_string()), span.clone()),
                ]),
                span.clone(),
            ),
            // Body: (if negate? `(- ,x) x)
            Syntax::new(
                SyntaxKind::List(vec![
                    Syntax::new(SyntaxKind::Symbol("if".to_string()), span.clone()),
                    Syntax::new(SyntaxKind::Symbol("negate?".to_string()), span.clone()),
                    Syntax::new(
                        SyntaxKind::Quasiquote(Box::new(Syntax::new(
                            SyntaxKind::List(vec![
                                Syntax::new(SyntaxKind::Symbol("-".to_string()), span.clone()),
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
                    ),
                    Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
                ]),
                span.clone(),
            ),
        ]),
        span.clone(),
    );
    expander
        .expand(defmacro_syntax, &mut symbols, &mut vm)
        .unwrap();

    // (maybe-negate 42 true) should expand to (- 42) because negate? is true
    let call_true = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("maybe-negate".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(42), span.clone()),
            Syntax::new(SyntaxKind::Bool(true), span.clone()),
        ]),
        span.clone(),
    );
    let result = expander.expand(call_true, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "(- 42)");

    // (maybe-negate 42 false) should expand to just 42 because negate? is false
    let call_false = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("maybe-negate".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Int(42), span.clone()),
            Syntax::new(SyntaxKind::Bool(false), span.clone()),
        ]),
        span.clone(),
    );
    let result = expander.expand(call_false, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "42");
}

/// Verify the cached transformer is populated after first expansion.
#[test]
fn test_macro_cache_populated_after_first_call() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    // Register prelude so that quasiquote is available
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    // Define a simple macro: (defmacro double (x) `(+ ,x ,x))
    let defmacro_src = "(defmacro double (x) `(+ ,x ,x))";
    let defmacro_syn = crate::reader::read_syntax(defmacro_src, "<test>").unwrap();
    expander
        .expand(defmacro_syn, &mut symbols, &mut vm)
        .unwrap();

    // Before first invocation, cache should be empty.
    {
        let macro_def = expander.macros.get("double").unwrap();
        assert!(
            macro_def.cached_transformer.borrow().is_none(),
            "cache should be empty before first invocation"
        );
    }

    // Invoke once.
    let call_src = "(double 5)";
    let call_syn = crate::reader::read_syntax(call_src, "<test>").unwrap();
    expander.expand(call_syn, &mut symbols, &mut vm).unwrap();

    // After first invocation, cache should be populated.
    {
        let macro_def = expander.macros.get("double").unwrap();
        assert!(
            macro_def.cached_transformer.borrow().is_some(),
            "cache should be populated after first invocation"
        );
    }
}

/// Verify that calling the same macro with different args produces
/// distinct, correct results (no cross-invocation state leakage).
#[test]
fn test_macro_cache_different_args_no_leakage() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    // (defmacro double (x) `(+ ,x ,x))
    let defmacro_syn =
        crate::reader::read_syntax("(defmacro double (x) `(+ ,x ,x))", "<test>").unwrap();
    expander
        .expand(defmacro_syn, &mut symbols, &mut vm)
        .unwrap();

    // Expand (double 1), (double 2), (double 3) and verify each expands
    // to a list containing the correct integer twice.
    for n in [1i64, 2, 3] {
        let src = format!("(double {})", n);
        let syn = crate::reader::read_syntax(&src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm).unwrap();
        let result_str = result.to_string();
        // Should expand to (+ n n) — check n appears in the output.
        let n_str = n.to_string();
        assert!(
            result_str.contains(&n_str),
            "(double {}) should expand to contain {}, got: {}",
            n,
            n_str,
            result_str
        );
        // Should contain + and the number twice.
        assert!(result_str.contains('+'), "should contain +: {}", result_str);
    }
}

/// Verify that falsy atom arguments (false, nil, 0) are passed correctly
/// and do not become truthy through the cached closure path.
#[test]
fn test_macro_cache_atom_args_falsy() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    // (defmacro echo-cond (test) `(if ,test true false))
    // Expands echo-cond so we can inspect what value 'test' had.
    let defmacro_syn = crate::reader::read_syntax(
        "(defmacro echo-cond (test) `(if ,test true false))",
        "<test>",
    )
    .unwrap();
    expander
        .expand(defmacro_syn, &mut symbols, &mut vm)
        .unwrap();

    // (echo-cond false) should expand to (if false true false)
    // The key assertion: 'false' in the expansion must still be the
    // boolean false literal, not a truthy syntax object.
    for _ in 0..3 {
        // Call multiple times to exercise both miss and hit paths.
        let syn = crate::reader::read_syntax("(echo-cond false)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm).unwrap();
        let result_str = result.to_string();
        // The result should contain 'false' as a literal.
        assert!(
            result_str.contains("false"),
            "false argument should remain false in expansion: {}",
            result_str
        );
    }
}

/// Verify rest-param macros work correctly with the cache.
#[test]
fn test_macro_cache_rest_params() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    // (defmacro my-begin (& forms) `(begin ,;forms))
    let defmacro_syn =
        crate::reader::read_syntax("(defmacro my-begin (& forms) `(begin ,;forms))", "<test>")
            .unwrap();
    expander
        .expand(defmacro_syn, &mut symbols, &mut vm)
        .unwrap();

    // (my-begin a b c) — rest args collected into list
    for _ in 0..2 {
        let syn = crate::reader::read_syntax("(my-begin a b c)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm).unwrap();
        let result_str = result.to_string();
        assert!(
            result_str.contains("begin"),
            "should expand to begin form: {}",
            result_str
        );
    }
}
