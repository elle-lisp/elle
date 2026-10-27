// audited: 2026-10-26
// The file-as-letrec pipeline, and what `import/load-file` reports when the file
// it loads fails.
//
// tests/AGENTS.md
//
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
    eval_file_source_with_stdlib(code, |r| {
        let result = r.unwrap();
        // log should be @[1, 2]
        let items = result.array_mut_read().expect("expected array");
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
    // compile_file returns one unit, not one per form.
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
// SECTION 0b: eval_file answers the last expression, and a load the one form
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
fn a_load_answers_the_module_form() {
    // tests/modules/test.lisp is one form, `(fn [] {...})`, so a load of it
    // answers that closure.
    let code = r#"(import/load-file "tests/modules/test.lisp")"#;
    eval_source(code, |r| {
        let result = r.unwrap();
        assert!(
            result.is_closure(),
            "import/load-file should return a closure, got {:?}",
            result
        );
    });
}

#[test]
fn a_call_of_the_loaded_closure_answers_the_exports() {
    let code = r#"
        (def exports ((import/load-file "tests/modules/test.lisp")))
        (get exports :test-var)
    "#;
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
}

#[test]
fn the_exports_of_a_load_destructure() {
    let code = r#"
        (def {:test-var tv :test-string ts}
          ((import/load-file "tests/modules/test.lisp")))
        (list tv ts)
    "#;
    eval_source(code, |r| {
        let result = r.unwrap();
        assert!(result.is_list(), "expected list, got {:?}", result);
    });
}

// ============================================================================
// SECTION 0b2: A second load must not break captured bindings
// ============================================================================
#[test]
fn a_second_load_does_not_corrupt_captured_bindings() {
    // A second load must not corrupt bindings captured by closures defined
    // before it. The counter-factual is a loader that answers an already-loaded
    // module with a boolean sentinel instead of the module's own value: calling
    // the result then fails with "Cannot call true". The module returns a
    // struct holding a function, and the closures that captured its bindings
    // must keep working after the second load.
    eval_source(
        r#"
        (def {:inc inc} ((import/load-file "./tests/modules/counter.lisp")))
        (defn check [] (assert (integer? (inc)) "captured binding still works"))
        (def _unused ((import/load-file "./tests/modules/counter.lisp")))
        (check)
        (assert (integer? (inc)) "direct call after second load")
        true
    "#,
        |result| assert_eq!(result.unwrap(), Value::bool(true)),
    );
}

// ============================================================================
// SECTION 0b3: A load that failed fails again, and a form cannot import
// ============================================================================

/// Run `code` and return the string it evaluates to, as owned data (the
/// runtime is torn down before the caller sees it).
fn evaluated_string(code: &str) -> String {
    eval_source(code, |r| {
        let value = r.unwrap_or_else(|e| panic!("the form itself signalled: {e}"));
        value
            .with_string(str::to_owned)
            .expect("the form should evaluate to a string")
    })
}

#[test]
fn a_module_that_failed_to_compile_reports_the_same_failure_on_the_next_load() {
    // The second load of a file that failed to compile reports that compile
    // failure again, and names the file it came from.
    let message = evaluated_string(
        r#"
        (let [first (protect (import/load-file "tests/modules/unbound-symbol.lisp"))
              second (protect (import/load-file "tests/modules/unbound-symbol.lisp"))]
          (assert (not (get first 0)) "the first load must fail to compile")
          (get (get second 1) :message))
    "#,
    );
    assert!(
        message.contains("undefined variable: host/no-such-primitive"),
        "the second load reports the compile failure: {message}"
    );
    assert!(
        message.contains("unbound-symbol.lisp:"),
        "the failure names the file and line: {message}"
    );
}

#[test]
fn a_module_whose_form_imports_is_refused() {
    // A module's form runs in a fiber that withholds :import, so a form that
    // imports, itself or any other module, fails the load. The counter-factual
    // let the form import, and a load started another load.
    let message = evaluated_string(
        r#"
        (let [r (protect (import/load-file "tests/modules/self-import.lisp"))]
          (assert (not (get r 0)) "the self-import must fail")
          (get (get r 1) :message))
    "#,
    );
    assert!(
        message.contains(":import"),
        "the error names the bit the form withholds: {message}"
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
