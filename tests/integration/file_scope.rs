// Integration tests for file-scope compilation (issue #469).
// Tests immutable vs mutable capture behavior at runtime.

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// SECTION 0: File-as-letrec pipeline (eval_file, compile_file, analyze_file)
// ============================================================================

/// Helper: evaluate source through the file-as-letrec pipeline.
fn eval_file_source(input: &str) -> Result<Value, String> {
    use elle::context::{set_symbol_table, set_vm_context};
    use elle::{register_primitives, SymbolTable, VM};

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _meta = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = elle::eval_file(input, &mut symbols, &mut vm, "<test>");
    set_vm_context(std::ptr::null_mut());
    result
}

/// Helper: compile source through the file-as-letrec pipeline.
fn compile_file_source(input: &str) -> Result<elle::CompileResult, String> {
    use elle::{register_primitives, SymbolTable, VM};

    let mut _vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _meta = register_primitives(&mut _vm, &mut symbols);
    elle::compile_file(input, &mut symbols, "<test>")
}

#[test]
fn test_file_single_def() {
    // A file with a single def returns the binding's value.
    assert_eq!(eval_file_source("(def x 42) x").unwrap(), Value::int(42));
}

#[test]
fn test_file_multiple_defs() {
    // Multiple defs, last expression is the return value.
    assert_eq!(
        eval_file_source("(def x 42) (def y (+ x 1)) y").unwrap(),
        Value::int(43)
    );
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
    assert_eq!(eval_file_source(code).unwrap(), Value::int(42));
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
    let result = eval_file_source(code).unwrap();
    // log should be @[1, 2]
    let items = result.as_array_mut().expect("expected array");
    let items = items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], Value::int(1));
    assert_eq!(items[1], Value::int(2));
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
    assert_eq!(
        eval_file_source("(var x 1) (assign x 2) x").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_file_var_set_from_later_expression() {
    // var can be assigned from a later bare expression.
    assert_eq!(
        eval_file_source("(var count 0) (assign count (+ count 1)) count").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_file_primitive_immutability() {
    // Primitives are immutable — (assign + 42) should fail.
    let result = compile_file_source("(assign + 42)");
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
    assert_eq!(
        eval_file_source("(def cons 42) cons").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_file_empty() {
    // Empty file returns nil.
    assert_eq!(eval_file_source("").unwrap(), Value::NIL);
}

#[test]
fn test_file_single_bare_expression() {
    // A single bare expression is the return value.
    assert_eq!(eval_file_source("(+ 1 2)").unwrap(), Value::int(3));
}

#[test]
fn test_file_destructuring_def() {
    // Destructuring def at file level.
    assert_eq!(
        eval_file_source("(def (a b) (list 10 20)) (+ a b)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_file_primitives_accessible() {
    // Primitives like + are accessible as lexical bindings.
    assert_eq!(eval_file_source("(+ 1 2 3)").unwrap(), Value::int(6));
}

#[test]
fn test_file_last_def_is_return() {
    // When the last form is a def, the file returns the def's value.
    assert_eq!(eval_file_source("(def x 42)").unwrap(), Value::int(42));
}

#[test]
fn test_file_compile_produces_single_result() {
    // compile_file returns a single CompileResult, not a Vec.
    let result = compile_file_source("(def x 1) (def y 2) (+ x y)");
    assert!(result.is_ok());
}

#[test]
fn test_file_analyze_produces_single_result() {
    // analyze_file returns a single AnalyzeResult.
    use elle::{register_primitives, SymbolTable, VM};

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _meta = register_primitives(&mut vm, &mut symbols);
    let result = elle::analyze_file("(def x 1) (def y 2)", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 0b: import-file returns file's last expression (Chunk 3 Part 1)
// ============================================================================

/// Helper: evaluate source through the file-as-letrec pipeline with stdlib.
fn eval_file_source_with_stdlib(input: &str) -> Result<Value, String> {
    use elle::context::{set_symbol_table, set_vm_context};
    use elle::{init_stdlib, register_primitives, SymbolTable, VM};

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _meta = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);
    let result = elle::eval_file(input, &mut symbols, &mut vm, "<test>");
    set_vm_context(std::ptr::null_mut());
    result
}

#[test]
fn test_eval_file_returns_last_expression() {
    // eval_file returns the value of the last expression in the file.
    assert_eq!(eval_file_source("(+ 1 2)").unwrap(), Value::int(3));
    assert_eq!(
        eval_file_source("(def x 10) (def y 20) (+ x y)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_eval_file_returns_closure_for_module() {
    // A file whose last expression is a closure returns that closure.
    let code = r#"
        (def x 42)
        (fn [] x)
    "#;
    let result = eval_file_source(code).unwrap();
    assert!(result.is_closure(), "expected closure, got {:?}", result);
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
    let result = eval_file_source(code).unwrap();
    // The result is a struct with :x and :y
    assert!(result.is_struct(), "expected struct, got {:?}", result);
}

#[test]
fn test_import_file_returns_closure() {
    // import-file on tests/modules/test.lisp returns a closure (the last
    // expression in the file). Under compile_file, the file's letrec body
    // is the last expression, which is `(fn [] {...})`.
    let code = r#"(import-file "tests/modules/test.lisp")"#;
    let result = eval_file_source_with_stdlib(code).unwrap();
    assert!(
        result.is_closure(),
        "import-file should return a closure, got {:?}",
        result
    );
}

#[test]
fn test_import_file_closure_returns_exports() {
    // Calling the closure returned by import-file yields the exports struct.
    let code = r#"
        (def exports ((import-file "tests/modules/test.lisp")))
        (get exports :test-var)
    "#;
    let result = eval_file_source_with_stdlib(code).unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_import_file_destructure_exports() {
    // Destructuring the closure result gives access to individual exports.
    let code = r#"
        (def {:test-var tv :test-string ts}
          ((import-file "tests/modules/test.lisp")))
        (list tv ts)
    "#;
    let result = eval_file_source_with_stdlib(code).unwrap();
    assert!(result.is_list(), "expected list, got {:?}", result);
}

// ============================================================================
// SECTION 0b2: Second import-file must not break captured bindings (issue #469)
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
    let result = eval_file_source_with_stdlib(
        r#"
        (def {:inc inc} ((import-file "./tests/modules/counter.lisp")))
        (defn check [] (assert (integer? (inc)) "captured binding still works"))
        (def _unused ((import-file "./tests/modules/counter.lisp")))
        (check)
        (assert (integer? (inc)) "direct call after second import")
        true
    "#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

// ============================================================================
// SECTION 0c: Destructured def bindings captured by closures (issue #469)
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
    let result = eval_file_source(code).unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_file_destructured_def_not_captured() {
    // Destructured def bindings at file level used directly (no capture).
    let code = r#"
        (def {:x x} {:x 42})
        x
    "#;
    let result = eval_file_source(code).unwrap();
    assert_eq!(result, Value::int(42));
}

// ============================================================================
// SECTION 1: Immutable captures (def) — no cell wrapping
// ============================================================================

#[test]
fn test_immutable_def_captured_by_closure() {
    // A def (immutable) binding captured by a closure should work correctly.
    // The value is captured by value, no LocalCell indirection.
    let code = r#"
        (begin
          (def x 42)
          (def f (fn () x))
          (f))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

#[test]
fn test_immutable_def_captured_nested() {
    // Immutable capture through multiple nesting levels.
    let code = r#"
        (begin
          (def x 10)
          (def f (fn () (fn () x)))
          ((f)))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(10));
}

#[test]
fn test_immutable_def_multiple_closures() {
    // Multiple closures capturing the same immutable binding.
    let code = r#"
        (begin
          (def x 5)
          (def f (fn () x))
          (def g (fn () (+ x x)))
          (+ (f) (g)))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(15));
}

#[test]
fn test_immutable_let_captured_by_closure() {
    // let bindings are immutable by default and captured by closures.
    let code = r#"
        (let ((x 99))
          (let ((f (fn () x)))
            (f)))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(99));
}

// ============================================================================
// SECTION 2: Mutable captures (var) — cell wrapping required
// ============================================================================

#[test]
fn test_mutable_var_captured_by_closure() {
    // A var (mutable) binding captured by a closure needs a cell.
    // The closure must see mutations.
    let code = r#"
        (begin
          (var x 1)
          (def f (fn () (begin (assign x 2) x)))
          (f))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(2));
}

#[test]
fn test_mutable_var_shared_between_closures() {
    // Two closures sharing a mutable capture via cell.
    let code = r#"
        (begin
          (var x 0)
          (def inc (fn () (assign x (+ x 1))))
          (def get (fn () x))
          (inc)
          (inc)
          (get))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(2));
}

#[test]
fn test_mutable_var_mutation_visible_after_call() {
    // Mutation through closure is visible in the enclosing scope.
    let code = r#"
        (begin
          (var x 0)
          (def inc (fn () (assign x (+ x 1))))
          (inc)
          (inc)
          (inc)
          x)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 3: Mixed immutable and mutable captures
// ============================================================================

#[test]
fn test_mixed_def_and_var_captures() {
    // A closure capturing both an immutable def and a mutable var.
    let code = r#"
        (begin
          (def base 10)
          (var count 0)
          (def f (fn () (begin (assign count (+ count 1)) (+ base count))))
          (f)
          (f)
          (f))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(13));
}

#[test]
fn test_def_fn_captured_by_sibling() {
    // A def'd function captured by a sibling function.
    let code = r#"
        (begin
          (def helper (fn (n) (+ n 1)))
          (def caller (fn (n) (helper n)))
          (caller 41))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

// ============================================================================
// Bug reproduction: eval with macros corrupting destructured binding cells
// ============================================================================

#[test]
fn test_file_destructure_eval_with_macro() {
    // Regression test: eval with a macro (like `when`) triggers macro expansion
    // which executes VM bytecode. Without stack save/restore around expansion,
    // the macro expansion overwrites the caller's local variable slots,
    // corrupting cells that hold destructured bindings.
    let result = eval_file_source(
        r#"
        (def {:f f} {:f (fn [a b c] a)})
        (defn helper [] (f 1 2 3))
        (f (eval '(when true 42)) 42 "test")
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

// ============================================================================
// Fixpoint signal propagation for mutually recursive file-scope lambdas
// ============================================================================

#[test]
fn test_mutual_recursion_signal_propagation() {
    // foo calls bar; bar yields; foo must also be inferred as Yields.
    // Without the fixpoint loop, foo is analyzed first and sees bar's
    // stale seed (Pure), so foo is incorrectly inferred as Pure.
    let result = eval_file_source(
        r#"
        (def foo (fn [] (bar)))
        (def bar (fn [] (yield 1) (foo)))
        (silent? foo)
        "#,
    );
    // foo is NOT silent — it calls a yielding function
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_mutual_recursion_signal_propagation_reverse_order() {
    // Same as above but bar is defined first — bar directly yields,
    // so foo should see bar's Yields signal even in a single pass.
    // This test ensures the fixpoint doesn't break the already-correct case.
    let result = eval_file_source(
        r#"
        (def bar (fn [] (yield 1) (foo)))
        (def foo (fn [] (bar)))
        (silent? foo)
        "#,
    );
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_mutual_recursion_three_way_signal_propagation() {
    // Three-way mutual recursion: a -> b -> c -> yield.
    // All three should be inferred as Yields.
    let result = eval_file_source(
        r#"
        (def a (fn [] (b)))
        (def b (fn [] (c)))
        (def c (fn [] (yield 1) (a)))
        (list (silent? a) (silent? b) (silent? c))
        "#,
    );
    let val = result.unwrap();
    // All three are NOT silent — they transitively call a yielding function
    let items = val.list_to_vec().expect("expected list");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], Value::bool(false), "a should not be silent");
    assert_eq!(items[1], Value::bool(false), "b should not be silent");
    assert_eq!(items[2], Value::bool(false), "c should not be silent");
}

#[test]
fn test_mutual_recursion_silent_stays_silent() {
    // Mutually recursive functions that are genuinely silent should stay silent.
    // The fixpoint must not incorrectly promote silent to Yields.
    let result = eval_file_source(
        r#"
        (def even? (fn [n] (if (= n 0) true (odd? (- n 1)))))
        (def odd? (fn [n] (if (= n 0) false (even? (- n 1)))))
        (list (silent? even?) (silent? odd?))
        "#,
    );
    let val = result.unwrap();
    let items = val.list_to_vec().expect("expected list");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], Value::bool(true), "even? should be silent");
    assert_eq!(items[1], Value::bool(true), "odd? should be silent");
}

// ============================================================================
// SECTION: import-file re-execution (Bug 1 — import-file must not cache)
// ============================================================================

#[test]
fn test_import_file_twice_reruns_module() {
    // Importing the same file twice should re-execute the module each time,
    // giving independent closures with independent mutable state.
    // If import-file caches, both imports share the same counter.
    let result = eval_file_source_with_stdlib(
        r#"
         (def {:inc inc1 :count count1} ((import-file "tests/modules/counter.lisp")))
         (def {:inc inc2 :count count2} ((import-file "tests/modules/counter.lisp")))
        (inc1)
        (inc1)
        (inc1)
        # If caching, inc2 shares state with inc1 and count2 would be 3
        (list (count1) (count2))
    "#,
    );
    let val = result.unwrap();
    let items = val.list_to_vec().expect("expected list");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], Value::int(3), "counter1 should be 3");
    assert_eq!(
        items[1],
        Value::int(0),
        "counter2 should be 0 (independent)"
    );
}
