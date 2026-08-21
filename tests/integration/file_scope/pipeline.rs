use super::*;

// ============================================================================
// SECTION 0: File-as-letrec pipeline (eval_file, compile_file, analyze_file)
// ============================================================================

#[test]
fn test_file_single_def() {
    // A file with a single def returns the binding's value.
    eval_file_source("(def x 42) x", |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn test_file_multiple_defs() {
    // Multiple defs, last expression is the return value.
    eval_file_source_with_stdlib("(def x 42) (def y (+ x 1)) y", |r| {
        assert_eq!(r.unwrap(), Value::int(43))
    });
}

#[test]
fn test_file_mutual_recursion() {
    // Mutual recursion between top-level defs works because letrec
    // pre-binds all names.
    let code = r#"
        (def f (fn () (g)))
        (def g (fn () 42))
        (f)
    "#;
    eval_file_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn test_file_side_effect_ordering() {
    // Side effects interleave correctly: initializers run sequentially.
    let code = r#"
        (var log @[])
        (def a (begin (push log 1) 1))
        (def b (begin (push log 2) 2))
        log
    "#;
    eval_file_source_stdlib(code, |r| {
        let result = r.unwrap();
        // log should be @[1, 2]
        let items = result.as_array_mut().expect("expected array");
        let items = items.borrow();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Value::int(1));
        assert_eq!(items[1], Value::int(2));
    });
}

#[test]
fn test_file_def_immutability() {
    // def bindings are immutable — (assign x ...) on a def should fail.
    let result = compile_file_source("(def x 1) (assign x 2)");
    assert!(result.is_err(), "expected compile error for assign on def");
    let err = result.unwrap_err();
    assert!(
        err.contains("immutable"),
        "error should mention immutable: {}",
        err
    );
}

#[test]
fn test_file_var_mutability() {
    // var bindings are mutable.
    eval_file_source("(var x 1) (assign x 2) x", |r| {
        assert_eq!(r.unwrap(), Value::int(2))
    });
}

#[test]
fn test_file_var_set_from_later_expression() {
    // var can be assigned from a later bare expression.
    eval_file_source_with_stdlib("(var count 0) (assign count (+ count 1)) count", |r| {
        assert_eq!(r.unwrap(), Value::int(1))
    });
}

#[test]
fn test_file_primitive_immutability() {
    // Primitives are immutable — (assign list 42) should fail.
    // Note: + moved to stdlib; use a primitive that remains (list).
    let result = compile_file_source("(assign list 42)");
    assert!(
        result.is_err(),
        "expected compile error for assign on primitive"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("immutable"),
        "error should mention immutable: {}",
        err
    );
}

#[test]
fn test_file_primitive_shadowing() {
    // File-level def can shadow a primitive.
    eval_file_source("(def pair 42) pair", |r| {
        assert_eq!(r.unwrap(), Value::int(42))
    });
}

#[test]
fn test_file_empty() {
    // Empty file returns nil.
    eval_file_source("", |r| assert_eq!(r.unwrap(), Value::NIL));
}

#[test]
fn test_file_single_bare_expression() {
    // A single bare expression is the return value.
    eval_file_source_with_stdlib("(+ 1 2)", |r| assert_eq!(r.unwrap(), Value::int(3)));
}

#[test]
fn test_file_destructuring_def() {
    // Destructuring def at file level.
    eval_file_source_with_stdlib("(def (a b) (list 10 20)) (+ a b)", |r| {
        assert_eq!(r.unwrap(), Value::int(30))
    });
}

#[test]
fn test_file_primitives_accessible() {
    // Primitives like + are accessible as lexical bindings.
    eval_file_source_with_stdlib("(+ 1 2 3)", |r| assert_eq!(r.unwrap(), Value::int(6)));
}

#[test]
fn test_file_last_def_is_return() {
    // When the last form is a def, the file returns the def's value.
    eval_file_source("(def x 42)", |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn test_file_compile_produces_single_result() {
    // compile_file returns a single CompileResult, not a Vec.
    let result = compile_file_source("(def x 1) (def y 2) (%add x y)");
    assert!(result.is_ok());
}

#[test]
fn test_file_analyze_produces_single_result() {
    // analyze_file returns a single AnalyzeResult.
    use elle::runtime::Runtime;

    let mut rt = Runtime::without_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = elle::analyze_file("(def x 1) (def y 2)", symbols, vm, cctx, "<test>");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 0b: import-file returns file's last expression
// ============================================================================
#[test]
fn test_eval_file_returns_last_expression() {
    // eval_file returns the value of the last expression in the file.
    eval_file_source_with_stdlib("(+ 1 2)", |r| assert_eq!(r.unwrap(), Value::int(3)));
    eval_file_source_with_stdlib("(def x 10) (def y 20) (+ x y)", |r| {
        assert_eq!(r.unwrap(), Value::int(30))
    });
}

#[test]
fn test_eval_file_returns_closure_for_module() {
    // A file whose last expression is a closure returns that closure.
    let code = r#"
        (def x 42)
        (fn [] x)
    "#;
    eval_file_source(code, |r| {
        let result = r.unwrap();
        assert!(result.is_closure(), "expected closure, got {:?}", result);
    });
}

#[test]
fn test_eval_file_module_closure_callable() {
    // The closure returned by eval_file can be called to get exports.
    let code = r#"
        (def x 42)
        (def y "hello")
        (def get-exports (fn [] {:x x :y y}))
        (get-exports)
    "#;
    eval_file_source(code, |r| {
        let result = r.unwrap();
        // The result is a struct with :x and :y
        assert!(result.is_struct(), "expected struct, got {:?}", result);
    });
}

#[test]
fn test_import_file_returns_closure() {
    // import-file on tests/modules/test.lisp returns a closure (the last
    // expression in the file). Under compile_file, the file's letrec body
    // is the last expression, which is `(fn [] {...})`.
    let code = r#"(import-file "tests/modules/test.lisp")"#;
    eval_file_source_with_stdlib(code, |r| {
        let result = r.unwrap();
        assert!(
            result.is_closure(),
            "import-file should return a closure, got {:?}",
            result
        );
    });
}

#[test]
fn test_import_file_closure_returns_exports() {
    // Calling the closure returned by import-file yields the exports struct.
    let code = r#"
        (def exports ((import-file "tests/modules/test.lisp")))
        (get exports :test-var)
    "#;
    eval_file_source_with_stdlib(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn test_import_file_destructure_exports() {
    // Destructuring the closure result gives access to individual exports.
    let code = r#"
        (def {:test-var tv :test-string ts}
          ((import-file "tests/modules/test.lisp")))
        (list tv ts)
    "#;
    eval_file_source_with_stdlib(code, |r| {
        let result = r.unwrap();
        assert!(result.is_list(), "expected list, got {:?}", result);
    });
}

// ============================================================================
// SECTION 0b2: Second import-file must not break captured bindings
// ============================================================================
#[test]
fn test_import_file_does_not_corrupt_captured_bindings() {
    // A second import-file call must not corrupt bindings captured by closures
    // defined before the import. The bug: import-file returned `true` (a
    // boolean sentinel) for already-loaded modules instead of the module's
    // cached return value. Calling `(true)` then failed with "Cannot call true".
    // Use a simple module that returns a struct with a function.
    // The test verifies that a second import-file call doesn't corrupt
    // closures that captured bindings from the first import.
    eval_file_source_with_stdlib(
        r#"
        (def {:inc inc} ((import-file "./tests/modules/counter.lisp")))
        (defn check [] (assert (integer? (inc)) "captured binding still works"))
        (def _unused ((import-file "./tests/modules/counter.lisp")))
        (check)
        (assert (integer? (inc)) "direct call after second import")
        true
    "#,
        |result| assert_eq!(result.unwrap(), Value::bool(true)),
    );
}

// ============================================================================
// SECTION 0c: Destructured def bindings captured by closures
// ============================================================================
#[test]
fn test_file_destructured_def_captured_by_closure() {
    // Destructured def bindings at file level should NOT get cell wrapping
    // even when captured by a closure. They are immutable.
    let code = r#"
        (def {:x x} {:x 42})
        (def f (fn [] x))
        (f)
    "#;
    eval_file_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn test_file_destructured_def_not_captured() {
    // Destructured def bindings at file level used directly (no capture).
    let code = r#"
        (def {:x x} {:x 42})
        x
    "#;
    eval_file_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
}
