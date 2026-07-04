//! defmacro registration, macro? predicate, and conditional-body tests.

use super::*;

#[test]
fn test_defmacro_registration() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

#[test]
fn test_defmacro_invalid_syntax() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

#[test]
fn test_defmacro_non_symbol_name() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

#[test]
fn test_macro_predicate_true() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 5, 1, 1);

        // Define a macro
        let macro_def = MacroDef {
            name: "my-macro".to_string(),
            params: vec!["x".to_string()],
            optional_params: vec![],
            rest_param: None,
            template: Syntax::new(SyntaxKind::Symbol("x".to_string()), span.clone()),
            definition_scope: ScopeId(0),
            cached_transformer: std::rc::Rc::new(RefCell::new(None)),
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
    });
}

#[test]
fn test_macro_predicate_false() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

#[test]
fn test_macro_predicate_non_symbol() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

#[test]
fn test_macro_predicate_wrong_arity() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}

/// Macro body uses `if` to conditionally generate different code.
/// This requires VM evaluation — template substitution can't do this.
#[test]
fn test_macro_with_conditional_body() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
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
    });
}
