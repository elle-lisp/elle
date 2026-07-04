use super::*;

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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(10)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(15)));
}

#[test]
fn test_immutable_let_captured_by_closure() {
    // let bindings are immutable by default and captured by closures.
    let code = r#"
        (let [x 99]
          (let [f (fn () x)]
            (f)))
    "#;
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(99)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(2)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(2)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(3)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(13)));
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
    eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
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
    eval_file_source(
        r#"
        (def {:f f} {:f (fn [a b c] a)})
        (defn helper [] (f 1 2 3))
        (f (eval '(when true 42)) 42 "test")
        "#,
        |r| assert_eq!(r.unwrap(), Value::int(42)),
    );
}

// ============================================================================
// Fixpoint signal propagation for mutually recursive file-scope lambdas
// ============================================================================
#[test]
fn test_mutual_recursion_signal_propagation() {
    // foo calls bar; bar yields; foo must also be inferred as Yields.
    // Without the fixpoint loop, foo is analyzed first and sees bar's
    // stale seed (Pure), so foo is incorrectly inferred as Pure.
    eval_file_source(
        r#"
        (def foo (fn [] (bar)))
        (def bar (fn [] (yield 1) (foo)))
        (silent? foo)
        "#,
        // foo is NOT silent — it calls a yielding function
        |result| assert_eq!(result.unwrap(), Value::bool(false)),
    );
}

#[test]
fn test_mutual_recursion_signal_propagation_reverse_order() {
    // Same as above but bar is defined first — bar directly yields,
    // so foo should see bar's Yields signal even in a single pass.
    // This test ensures the fixpoint doesn't break the already-correct case.
    eval_file_source(
        r#"
        (def bar (fn [] (yield 1) (foo)))
        (def foo (fn [] (bar)))
        (silent? foo)
        "#,
        |result| assert_eq!(result.unwrap(), Value::bool(false)),
    );
}

#[test]
fn test_mutual_recursion_three_way_signal_propagation() {
    // Three-way mutual recursion: a -> b -> c -> yield.
    // All three should be inferred as Yields.
    eval_file_source(
        r#"
        (def a (fn [] (b)))
        (def b (fn [] (c)))
        (def c (fn [] (yield 1) (a)))
        (list (silent? a) (silent? b) (silent? c))
        "#,
        |result| {
            let val = result.unwrap();
            // All three are NOT silent — they transitively call a yielding function
            let items = val.list_to_vec().expect("expected list");
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Value::bool(false), "a should not be silent");
            assert_eq!(items[1], Value::bool(false), "b should not be silent");
            assert_eq!(items[2], Value::bool(false), "c should not be silent");
        },
    );
}

#[test]
fn test_mutual_recursion_silent_stays_silent() {
    // Mutually recursive functions that are genuinely silent should stay silent.
    // The fixpoint must not incorrectly promote silent to Yields.
    eval_file_source(
        r#"
        (def even? (fn [n] (if (%eq n 0) true (odd? (%sub n 1)))))
        (def odd? (fn [n] (if (%eq n 0) false (even? (%sub n 1)))))
        (list (silent? even?) (silent? odd?))
        "#,
        |result| {
            let val = result.unwrap();
            let items = val.list_to_vec().expect("expected list");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::bool(true), "even? should be silent");
            assert_eq!(items[1], Value::bool(true), "odd? should be silent");
        },
    );
}

// ============================================================================
// SECTION: import-file re-execution (import-file must not cache)
// ============================================================================
#[test]
fn test_import_file_twice_reruns_module() {
    // Importing the same file twice should re-execute the module each time,
    // giving independent closures with independent mutable state.
    // If import-file caches, both imports share the same counter.
    eval_file_source_with_stdlib(
        r#"
         (def {:inc inc1 :count count1} ((import-file "tests/modules/counter.lisp")))
         (def {:inc inc2 :count count2} ((import-file "tests/modules/counter.lisp")))
        (inc1)
        (inc1)
        (inc1)
        # If caching, inc2 shares state with inc1 and count2 would be 3
        (list (count1) (count2))
    "#,
        |result| {
            let val = result.unwrap();
            let items = val.list_to_vec().expect("expected list");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::int(3), "counter1 should be 3");
            assert_eq!(
                items[1],
                Value::int(0),
                "counter2 should be 0 (independent)"
            );
        },
    );
}
