//! expand-macro primitive and transformer-cache behavior tests.

use super::*;

#[test]
fn test_expand_macro_basic() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // Define a macro: (defmacro double (x) `(+ ,x ,x))
        let defmacro = read_syntax(arena, "(defmacro double (x) `(+ ,x ,x))", "<test>").unwrap();
        expander.expand(defmacro, &mut symbols, &mut vm).unwrap();

        // (expand-macro '(double 5)) should return '(+ 5 5)
        let expand_call = read_syntax(arena, "(expand-macro '(double 5))", "<test>").unwrap();

        let result = expander.expand(expand_call, &mut symbols, &mut vm);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be a quoted form: '(+ 5 5)
        assert_eq!(expanded.to_string(), "'(+ 5 5)");
    });
}

#[test]
fn test_expand_macro_non_macro() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // (expand-macro '(+ 1 2)) should return '(+ 1 2) unchanged
        let expand_call = read_syntax(arena, "(expand-macro '(+ 1 2))", "<test>").unwrap();

        let result = expander.expand(expand_call, &mut symbols, &mut vm);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be unchanged: '(+ 1 2)
        assert_eq!(expanded.to_string(), "'(+ 1 2)");
    });
}

#[test]
fn test_expand_macro_wrong_arity() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // (expand-macro) with no arguments should error
        let expand_call = read_syntax(arena, "(expand-macro)", "<test>").unwrap();

        let result = expander.expand(expand_call, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires exactly 1 argument"));
    });
}

#[test]
fn test_expand_macro_unquoted_arg() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // (expand-macro x) with unquoted arg returns the arg unchanged
        let expand_call = read_syntax(arena, "(expand-macro x)", "<test>").unwrap();

        let result = expander.expand(expand_call, &mut symbols, &mut vm);
        assert!(result.is_ok());
        let expanded = result.unwrap();
        // Result should be the symbol x unchanged
        assert_eq!(expanded.to_string(), "x");
    });
}

/// Verify the cached transformer is populated after first expansion.
#[test]
fn test_macro_cache_populated_after_first_call() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        // Register prelude so that quasiquote is available
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // Define a simple macro: (defmacro double (x) `(+ ,x ,x))
        let defmacro_src = "(defmacro double (x) `(+ ,x ,x))";
        let defmacro_syn = read_syntax(arena, defmacro_src, "<test>").unwrap();
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
        let call_syn = read_syntax(arena, call_src, "<test>").unwrap();
        expander.expand(call_syn, &mut symbols, &mut vm).unwrap();

        // After first invocation, cache should be populated.
        {
            let macro_def = expander.macros.get("double").unwrap();
            assert!(
                macro_def.cached_transformer.borrow().is_some(),
                "cache should be populated after first invocation"
            );
        }
    });
}

/// Verify that calling the same macro with different args produces
/// distinct, correct results (no cross-invocation state leakage).
#[test]
fn test_macro_cache_different_args_no_leakage() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // (defmacro double (x) `(+ ,x ,x))
        let defmacro_syn =
            read_syntax(arena, "(defmacro double (x) `(+ ,x ,x))", "<test>").unwrap();
        expander
            .expand(defmacro_syn, &mut symbols, &mut vm)
            .unwrap();

        // Expand (double 1), (double 2), (double 3) and verify each expands
        // to a list containing the correct integer twice.
        for n in [1i64, 2, 3] {
            let src = format!("(double {})", n);
            let syn = read_syntax(arena, &src, "<test>").unwrap();
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
    });
}

/// Verify that falsy atom arguments (false, nil, 0) are passed correctly
/// and do not become truthy through the cached closure path.
#[test]
fn test_macro_cache_atom_args_falsy() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // (defmacro echo-cond (test) `(if ,test true false))
        // Expands echo-cond so we can inspect what value 'test' had.
        let defmacro_syn = read_syntax(
            arena,
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
            let syn = read_syntax(arena, "(echo-cond false)", "<test>").unwrap();
            let result = expander.expand(syn, &mut symbols, &mut vm).unwrap();
            let result_str = result.to_string();
            // The result should contain 'false' as a literal.
            assert!(
                result_str.contains("false"),
                "false argument should remain false in expansion: {}",
                result_str
            );
        }
    });
}

/// Verify rest-param macros work correctly with the cache.
/// Uses compile_file to ensure core.lisp (with `append`) is loaded,
/// since splice (,;) expands to append calls.
#[test]
fn test_macro_cache_rest_params() {
    crate::value::arena::with_test_region(|| {
        let mut symbols = SymbolTable::new();
        let source = r#"
        (defmacro my-begin (& forms) `(begin ,;forms))
        (my-begin 1 2 3)
    "#;
        let mut cctx = crate::pipeline::CompileCtx::new();
        let result = crate::pipeline::compile_file(source, &mut symbols, &mut cctx, "<test>");
        assert!(
            result.is_ok(),
            "rest-param macro with splice should compile: {:?}",
            result.err()
        );
    });
}

/// Counterfactual: macro-arg wrapping must not leak. The
/// expansion wraps its args into an explicit per-expansion transient region
/// (`expand_macro_call`) and the result is deep-copied to Syntax via
/// `from_value` before that region frees, so each wrapped compound arg is an
/// ordinary mortal allocation reclaimed with the transient region. After warming
/// the transformer
/// cache, repeated expansion must leave the live heap-object count flat; the old
/// per-expansion leak grew it ~1 object/expansion.
#[test]
fn test_macro_arg_wrapping_does_not_leak() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        let span = Span::new(0, 5, 1, 1);

        // (defmacro idmac (x) x) — an identity template, so each expansion's only
        // per-call allocation is the `wrap_macro_arg_value` wrapper for the arg.
        let macro_def = MacroDef {
            name: "idmac".to_string(),
            params: vec!["x".to_string()],
            optional_params: vec![],
            rest_param: None,
            template: Syntax::symbol(&arena, "x", span),
            cached_transformer: std::rc::Rc::new(RefCell::new(None)),
        };
        expander.define_macro(macro_def);

        // A macro call with a COMPOUND arg `(foo)` — wrap_macro_arg_value takes the
        // `_ => Value::syntax(...)` arm (an immediate arg would never allocate).
        let make_call = || read_syntax(arena, "(idmac (foo))", "<test>").unwrap();

        // Warm up: the first expansion compiles + caches the transformer (one-time
        // resident constants) so the measured window isolates wrap_macro_arg_value.
        expander.expand(make_call(), &mut symbols, &mut vm).unwrap();

        // Macro expansion allocates into the VM's heap (it is threaded through
        // `expander.expand(.., &mut vm)`), so measure THAT heap's live-object
        // count before and after — a fresh heap would always read 0 and make the
        // leak assertion meaningless.
        let before = vm.heap().len() as i64;
        let n = 200i64;
        for _ in 0..n {
            expander.expand(make_call(), &mut symbols, &mut vm).unwrap();
        }
        let delta = vm.heap().len() as i64 - before;

        assert!(
            delta < n / 4,
            "macro-arg wrapping leaks: {n} expansions grew the live heap by {delta} \
             (a bounded, transient-region wrapping would stay flat)"
        );
    });
}
