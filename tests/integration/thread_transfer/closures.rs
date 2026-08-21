use super::*;

// ============================================================================
// Test 3: Successful spawned closure still works
// ============================================================================

#[test]
fn test_spawned_closure_success() {
    // Even successful closures should work correctly
    let result = eval_source(
        r#"
        (let [x 10 y 20]
          (sys/join (sys/spawn-vm (fn () (+ x y)))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(30));
}
// ============================================================================
// Test 9: Spawned closure with captured computation
// ============================================================================

#[test]
fn test_spawned_closure_captured_computation() {
    // Closure captures computed values
    let result = eval_source(
        r#"
        (let [x (+ 1 2)
              y (* 3 4)]
          (sys/join (sys/spawn-vm (fn () (+ x y)))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(15)); // 3 + 12 = 15
}

// ============================================================================
// Test 10: Spawned closure with conditional
// ============================================================================

#[test]
fn test_spawned_closure_with_conditional() {
    // Closure uses conditional logic
    let result = eval_source(
        r#"
        (let [x 10]
          (sys/join (sys/spawn-vm (fn () (if (> x 5) "big" "small")))))
        "#,
        |r| r.map(|v| v.with_string(|s| s == "big")),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(true));
}

// ============================================================================
// Test 11: Verify closure constants are transferred
// ============================================================================

#[test]
fn test_spawned_closure_constants_transferred() {
    // Closure uses constants (literals in the body)
    let result = eval_source(
        r#"
        (sys/join (sys/spawn-vm (fn () (+ 100 200))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(300));
}

// ============================================================================
// Test 12: Verify closure with string constant
// ============================================================================

#[test]
fn test_spawned_closure_string_constant() {
    // Closure returns a string constant
    let result = eval_source(
        r#"
        (sys/join (sys/spawn-vm (fn () "hello from thread")))
        "#,
        |r| r.map(|v| v.with_string(|s| s == "hello from thread")),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(true));
}
// ============================================================================
// Test 15: Spawned closure with array operations
// ============================================================================

#[test]
fn test_spawned_closure_array_operations() {
    // Closure performs array operations
    let result = eval_source(
        r#"
        (let [v @[1 2 3]]
          (sys/join (sys/spawn-vm (fn () (get v 1)))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap(), Some(2));
}
// ============================================================================
// Test 16: Closure capturing another closure
// ============================================================================

#[test]
fn test_closure_capturing_closure() {
    let result = eval_source(
        r#"
        (let [add1 (fn (x) (+ x 1))]
          (sys/join (sys/spawn-vm (fn () (add1 41)))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some(42));
}

// ============================================================================
// Test 17: Closure capturing nested closures (three levels)
// ============================================================================

#[test]
fn test_closure_capturing_nested_closures() {
    let result = eval_source(
        r#"
        (let [add1 (fn (x) (+ x 1))]
          (let [add2 (fn (x) (add1 (add1 x)))]
            (sys/join (sys/spawn-vm (fn () (add2 40))))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some(42));
}

// ============================================================================
// Test 18: Closure capturing non-sendable value via inner closure is rejected
// ============================================================================

#[test]
fn test_closure_capturing_struct_mut_sendable() {
    // A closure that captures a mutable @struct (via an inner closure) is now sendable.
    let result = eval_source(
        r#"
        (let [t (@struct :x 42)]
          (let [f (fn () (t :x))]
            (sys/join (sys/spawn-vm (fn () (f))))))
        "#,
        |r| r.map(|_| ()),
    );

    // spawn should succeed — @struct is sendable.
    assert!(
        result.is_ok(),
        "Expected spawn to succeed for @struct capture: {:?}",
        result
    );
}

// ============================================================================
// Test 19: Spawned closure returning a closure as its result
// ============================================================================

#[test]
fn test_closure_result_is_closure() {
    let result = eval_source(
        r#"
        (let [f (sys/join (sys/spawn-vm (fn () (fn (x) (+ x 1)))))]
          (f 41))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some(42));
}

// ============================================================================
// Test 20: Self-recursive closure via letrec (factorial)
// ============================================================================

#[test]
fn test_self_recursive_closure() {
    let result = eval_source(
        r#"
        (letrec [fact (fn (n) (if (= n 0) 1 (* n (fact (- n 1)))))]
          (sys/join (sys/spawn-vm (fn () (fact 5)))))
        "#,
        |r| r.map(|v| v.as_int()),
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some(120));
}

// ============================================================================
// Test 21: Mutually recursive closures via letrec (even?/odd?)
// ============================================================================

#[test]
fn test_mutually_recursive_closures() {
    let result = eval_source(
        r#"
        (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                 odd?  (fn (n) (if (= n 0) false (even? (- n 1))))]
          (sys/join (sys/spawn-vm (fn () (even? 10)))))
        "#,
        |r| r.map(|v| v.as_bool()),
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some(true));
}
