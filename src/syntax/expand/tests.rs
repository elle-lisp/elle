//! Tests for macro expansion

use super::*;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax, SyntaxKind};
use crate::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
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
    // Should expand to (list (quote a) (quote b) (quote c))
    let result_str = result.to_string();
    assert!(
        result_str.contains("list"),
        "Result should contain 'list': {}",
        result_str
    );
    assert!(
        result_str.contains("quote"),
        "Result should contain 'quote': {}",
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
    let result_str = result.to_string();
    assert!(
        result_str.contains("list"),
        "Result should contain 'list': {}",
        result_str
    );
    assert!(
        result_str.contains("quote"),
        "Result should contain 'quote': {}",
        result_str
    );
    assert!(
        result_str.contains("x"),
        "Result should contain 'x': {}",
        result_str
    );
}

#[test]
fn test_quasiquote_with_splicing() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 10, 1, 1);

    // `(a ,@xs b)
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
    // Should expand to (quote x)
    assert!(
        result_str.contains("quote"),
        "Result should contain 'quote': {}",
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
        template: Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
        definition_scope: ScopeId(0),
    };
    expander.define_macro(macro_def);

    // (macro? my-macro) should return #t
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
    assert_eq!(expanded.to_string(), "#t");
}

#[test]
fn test_macro_predicate_false() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (macro? not-a-macro) should return #f
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
    assert_eq!(expanded.to_string(), "#f");
}

#[test]
fn test_macro_predicate_non_symbol() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (macro? 42) should return #f (not a symbol)
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
    assert_eq!(expanded.to_string(), "#f");
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
        template,
        definition_scope: ScopeId(0),
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
fn test_qualified_symbol_string_module() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // string:upcase should expand to string-upcase
    let syntax = Syntax::new(
        SyntaxKind::Symbol("string:upcase".to_string()),
        span.clone(),
    );
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "string-upcase");

    // string:length should expand to string-length
    let syntax = Syntax::new(SyntaxKind::Symbol("string:length".to_string()), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "string-length");
}

#[test]
fn test_qualified_symbol_math_module() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // math:abs should expand to abs
    let syntax = Syntax::new(SyntaxKind::Symbol("math:abs".to_string()), span.clone());
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "abs");

    // math:floor should expand to floor
    let syntax = Syntax::new(SyntaxKind::Symbol("math:floor".to_string()), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "floor");
}

#[test]
fn test_qualified_symbol_list_module() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // list:length should expand to length
    let syntax = Syntax::new(SyntaxKind::Symbol("list:length".to_string()), span.clone());
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "length");

    // list:append should expand to append
    let syntax = Syntax::new(SyntaxKind::Symbol("list:append".to_string()), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "append");
}

#[test]
fn test_qualified_symbol_in_call() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // (string:upcase "hello") should expand to (string-upcase "hello")
    let syntax = Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(
                SyntaxKind::Symbol("string:upcase".to_string()),
                span.clone(),
            ),
            Syntax::new(SyntaxKind::String("hello".to_string()), span.clone()),
        ]),
        span,
    );
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "(string-upcase \"hello\")");
}

#[test]
fn test_qualified_symbol_unknown_module() {
    let mut expander = Expander::new();
    let (mut symbols, mut vm) = setup();
    let span = Span::new(0, 5, 1, 1);

    // unknown:foo should remain unchanged (unknown module)
    let syntax = Syntax::new(SyntaxKind::Symbol("unknown:foo".to_string()), span);
    let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
    assert_eq!(result.to_string(), "unknown:foo");
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

    // (maybe-negate 42 #t) should expand to (- 42) because negate? is #t
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

    // (maybe-negate 42 #f) should expand to just 42 because negate? is #f
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
