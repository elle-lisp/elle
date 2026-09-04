//! defmacro registration, macro? predicate, and conditional-body tests.

use super::*;

/// Expand `src` and return the rendered result.
fn expand_str(src: &str) -> Result<String, String> {
    let (mut expander, mut symbols, mut vm, arena) = setup();
    let form = read_syntax(arena, src, "<test>").expect("test source parses");
    expander
        .expand(form, &mut symbols, &mut vm)
        .map(|s| s.to_string())
}

#[test]
fn test_defmacro_registration() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        let defmacro_form =
            read_syntax(arena, "(defmacro double (x) `(* ,x 2))", "<test>").unwrap();
        let expanded = expander
            .expand(defmacro_form, &mut symbols, &mut vm)
            .expect("defmacro expands");
        // defmacro should expand to nil
        assert_eq!(expanded.to_string(), "nil");

        // Now use the macro: (double 21)
        let macro_call = read_syntax(arena, "(double 21)", "<test>").unwrap();
        let expanded = expander
            .expand(macro_call, &mut symbols, &mut vm)
            .expect("macro call expands");
        assert_eq!(expanded.to_string(), "(* 21 2)");
    });
}

#[test]
fn test_defmacro_invalid_syntax() {
    crate::value::arena::with_test_region(|| {
        // defmacro with wrong number of arguments
        let err = expand_str("(defmacro double)").unwrap_err();
        assert!(err.contains("requires exactly 3 arguments"), "{}", err);
    });
}

#[test]
fn test_defmacro_non_symbol_name() {
    crate::value::arena::with_test_region(|| {
        let err = expand_str("(defmacro 42 () x)").unwrap_err();
        assert!(err.contains("macro name must be a symbol"), "{}", err);
    });
}

#[test]
fn test_macro_predicate_true() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        let span = Span::new(0, 5, 1, 1);

        // Define a macro
        let macro_def = MacroDef {
            name: "my-macro".to_string(),
            params: vec!["x".to_string()],
            optional_params: vec![],
            rest_param: None,
            template: Syntax::symbol(&arena, "x", span),
            cached_transformer: std::rc::Rc::new(RefCell::new(None)),
        };
        expander.define_macro(macro_def);

        // (macro? my-macro) should return true
        let check = read_syntax(arena, "(macro? my-macro)", "<test>").unwrap();
        let expanded = expander
            .expand(check, &mut symbols, &mut vm)
            .expect("macro? expands");
        assert_eq!(expanded.to_string(), "true");
    });
}

#[test]
fn test_macro_predicate_false() {
    crate::value::arena::with_test_region(|| {
        assert_eq!(expand_str("(macro? not-a-macro)").unwrap(), "false");
    });
}

#[test]
fn test_macro_predicate_non_symbol() {
    crate::value::arena::with_test_region(|| {
        // (macro? 42) should return false (not a symbol)
        assert_eq!(expand_str("(macro? 42)").unwrap(), "false");
    });
}

#[test]
fn test_macro_predicate_wrong_arity() {
    crate::value::arena::with_test_region(|| {
        let err = expand_str("(macro?)").unwrap_err();
        assert!(err.contains("requires exactly 1 argument"), "{}", err);
    });
}

/// Macro body uses `if` to conditionally generate different code.
/// This requires VM evaluation — template substitution can't do this.
#[test]
fn test_macro_with_conditional_body() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        let defmacro_syntax = read_syntax(
            arena,
            "(defmacro maybe-negate (x negate?) (if negate? `(- ,x) x))",
            "<test>",
        )
        .unwrap();
        expander
            .expand(defmacro_syntax, &mut symbols, &mut vm)
            .unwrap();

        // (maybe-negate 42 true) should expand to (- 42) because negate? is true
        let call_true = read_syntax(arena, "(maybe-negate 42 true)", "<test>").unwrap();
        let result = expander.expand(call_true, &mut symbols, &mut vm).unwrap();
        assert_eq!(result.to_string(), "(- 42)");

        // (maybe-negate 42 false) should expand to just 42 because negate? is false
        let call_false = read_syntax(arena, "(maybe-negate 42 false)", "<test>").unwrap();
        let result = expander.expand(call_false, &mut symbols, &mut vm).unwrap();
        assert_eq!(result.to_string(), "42");
    });
}
