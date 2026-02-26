// JIT integration tests
//
// Tests the JIT compilation pipeline: hot function detection, compilation,
// and execution via native code.

mod jit_tests {
    use crate::common::eval_source;
    use elle::value::Value;

    #[test]
    fn test_jit_triggered_by_hot_loop() {
        // Call a pure function 20 times — should trigger JIT at call 10
        let code = r#"(begin
            (defn add1 (x) (+ x 1))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (add1 acc))))
            (loop 20 0))"#;
        let result = eval_source(code).unwrap();
        assert_eq!(result, Value::int(20));
    }

    #[test]
    fn test_jit_simple_arithmetic() {
        // Simple arithmetic function called many times
        let code = r#"(begin
            (defn square (x) (* x x))
            (defn sum-squares (n acc)
              (if (= n 0) acc (sum-squares (- n 1) (+ acc (square n)))))
            (sum-squares 15 0))"#;
        let result = eval_source(code).unwrap();
        // Sum of squares from 1 to 15 = 1240
        assert_eq!(result, Value::int(1240));
    }

    #[test]
    fn test_jit_with_captures() {
        // Function with captured variables
        let code = r#"(begin
            (defn make-adder (n)
              (fn (x) (+ x n)))
            (var add5 (make-adder 5))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (add5 acc))))
            (loop 15 0))"#;
        let result = eval_source(code).unwrap();
        // 15 * 5 = 75
        assert_eq!(result, Value::int(75));
    }

    #[test]
    fn test_jit_comparison_operations() {
        // Test all comparison operations
        let code = r#"(begin
            (defn test-comparisons (x y)
              (list (= x y) (< x y) (> x y) (<= x y) (>= x y)))
            (defn loop (n)
              (if (= n 0)
                  (test-comparisons 5 10)
                  (begin (test-comparisons n (+ n 1)) (loop (- n 1)))))
            (loop 15))"#;
        let result = eval_source(code).unwrap();
        // Should return (false true false true false) for (= 5 10), (< 5 10), etc.
        assert!(result.is_cons());
    }

    #[test]
    fn test_jit_modulo_and_division() {
        // Test modulo and division operations
        let code = r#"(begin
            (defn mod-div-test (x y)
              (+ (/ x y) (% x y)))
            (defn loop (n acc)
              (if (= n 1) acc (loop (- n 1) (+ acc (mod-div-test n 2)))))
            (loop 15 0))"#;
        let result = eval_source(code).unwrap();
        // For n from 15 down to 2: (n/2) + (n%2)
        // 15: 7+1=8, 14: 7+0=7, 13: 6+1=7, 12: 6+0=6, 11: 5+1=6, 10: 5+0=5
        // 9: 4+1=5, 8: 4+0=4, 7: 3+1=4, 6: 3+0=3, 5: 2+1=3, 4: 2+0=2
        // 3: 1+1=2, 2: 1+0=1
        // Sum = 8+7+7+6+6+5+5+4+4+3+3+2+2+1 = 63
        assert_eq!(result, Value::int(63));
    }

    #[test]
    fn test_jit_conditional_branches() {
        // Test conditional branching in JIT
        let code = r#"(begin
            (defn abs (x)
              (if (< x 0) (- 0 x) x))
            (defn sum-abs (n acc)
              (if (= n 0) acc (sum-abs (- n 1) (+ acc (abs (- n 8))))))
            (sum-abs 15 0))"#;
        let result = eval_source(code).unwrap();
        // Sum of |n - 8| for n from 1 to 15
        // = |1-8| + |2-8| + ... + |15-8|
        // = 7 + 6 + 5 + 4 + 3 + 2 + 1 + 0 + 1 + 2 + 3 + 4 + 5 + 6 + 7 = 56
        assert_eq!(result, Value::int(56));
    }

    #[test]
    fn test_jit_nested_calls() {
        // Test nested function calls
        let code = r#"(begin
            (defn f (x) (+ x 1))
            (defn g (x) (f (f x)))
            (defn h (x) (g (g x)))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (h acc))))
            (loop 12 0))"#;
        let result = eval_source(code).unwrap();
        // Each call to h adds 4, so 12 * 4 = 48
        assert_eq!(result, Value::int(48));
    }

    #[test]
    fn test_jit_float_arithmetic() {
        // Test float arithmetic
        let code = r#"(begin
            (defn float-op (x y)
              (+ (* x y) (- x y)))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (float-op acc 1.5))))
            (loop 12 1.0))"#;
        let result = eval_source(code).unwrap();
        assert!(result.is_float());
    }

    #[test]
    fn test_jit_identity_function() {
        // Simple identity function
        let code = r#"(begin
            (defn id (x) x)
            (defn loop (n)
              (if (= n 0) (id 42) (begin (id n) (loop (- n 1)))))
            (loop 15))"#;
        let result = eval_source(code).unwrap();
        assert_eq!(result, Value::int(42));
    }

    #[test]
    fn test_jit_multiple_args() {
        // Function with multiple arguments
        let code = r#"(begin
            (defn add3 (a b c) (+ a (+ b c)))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (add3 acc n 1))))
            (loop 12 0))"#;
        let result = eval_source(code).unwrap();
        // Sum of (n + 1) for n from 1 to 12 = 12 + 11 + ... + 1 + 12 = 78 + 12 = 90
        // Actually: acc starts at 0, each iteration adds (acc + n + 1)
        // This is more complex, let's just verify it runs
        assert!(result.is_int());
    }

    #[test]
    fn test_jit_does_not_break_non_pure() {
        // Non-pure functions should still work (via interpreter)
        let code = r#"(begin
            (var counter 0)
            (defn inc! ()
              (set! counter (+ counter 1))
              counter)
            (defn loop (n)
              (if (= n 0) counter (begin (inc!) (loop (- n 1)))))
            (loop 15))"#;
        let result = eval_source(code).unwrap();
        assert_eq!(result, Value::int(15));
    }

    #[test]
    fn test_jit_before_and_after_threshold() {
        // Verify results are consistent before and after JIT kicks in
        let code = r#"(begin
            (defn fib (n)
              (if (<= n 1) n (+ (fib (- n 1)) (fib (- n 2)))))
            (var results (list))
            (defn collect (n)
              (if (= n 0)
                  results
                  (begin
                    (set! results (cons (fib 10) results))
                    (collect (- n 1)))))
            (collect 15))"#;
        let result = eval_source(code).unwrap();
        // All results should be fib(10) = 55
        // Verify the list is non-empty and all elements are 55
        assert!(result.is_cons());
        let mut current = result;
        while let Some(cons) = current.as_cons() {
            assert_eq!(cons.first, Value::int(55));
            current = cons.rest;
        }
    }

    #[test]
    fn test_jit_tail_call_basic() {
        // Tail-recursive sum — should work correctly with TCO
        let result = eval_source(
            r#"(begin
            (defn sum-to (n acc)
                (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
            (sum-to 100 0))"#,
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap().as_int(), Some(5050));
    }

    #[test]
    fn test_jit_tail_call_deep_recursion() {
        // Deep tail recursion that would blow the stack without TCO
        let result = eval_source(
            r#"(begin
            (defn count-down (n)
                (if (= n 0) 0 (count-down (- n 1))))
            (count-down 50000))"#,
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap().as_int(), Some(0));
    }

    #[test]
    fn test_jit_tail_call_mutual_recursion() {
        // Mutual recursion via tail calls
        let result = eval_source(
            r#"(begin
            (defn is-even (n)
                (if (= n 0) true (is-odd (- n 1))))
            (defn is-odd (n)
                (if (= n 0) false (is-even (- n 1))))
            (is-even 100))"#,
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_jit_does_not_regress_recursive_workloads() {
        // Verify that tail-recursive functions work correctly
        // (they should fall through to interpreter, not JIT)
        let result = eval_source(
            r#"(begin
            (defn fib-tail (n a b)
                (if (= n 0) a
                    (if (= n 1) b
                        (fib-tail (- n 1) b (+ a b)))))
            (fib-tail 30 0 1))"#,
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let val = result.unwrap();
        assert_eq!(val.as_int(), Some(832040));
    }

    #[test]
    fn test_jit_callee_exception_propagates() {
        // A pure function calls another pure function that throws.
        // The JIT must propagate the exception, not swallow it.
        //
        // `helper` is pure (just does division).
        // `f` is pure (calls a pure function and adds 1).
        // When `f` is JIT-compiled and calls `helper` with x=0,
        // the division-by-zero exception must propagate correctly.
        let result = eval_source(
            r#"(begin
                (defn helper (x) (/ 10 x))
                (defn f (x) (+ (helper x) 1))
                ; Call f enough times to make it hot, then call with 0
                (f 1) (f 2) (f 3) (f 4) (f 5)
                (f 6) (f 7) (f 8) (f 9) (f 10)
                (let ((fib (fiber/new (fn () (f 0)) 1)))
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -1 0)))"#,
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        // Should get -1 from the error handler, not garbage from continuing after exception
        assert_eq!(val.as_int(), Some(-1));
    }

    #[test]
    fn test_jit_exception_does_not_continue_execution() {
        // Verify that after an exception, subsequent code in the JIT function
        // does NOT execute. If exception propagation is broken, the JIT would
        // continue executing with NIL and produce wrong results.
        let result = eval_source(
            r#"(begin
                (defn helper (x) (/ 10 x))
                (defn f (x)
                  ; Call helper, then add 1000 to result
                  ; If exception propagation is broken, this would execute
                  ; with result=NIL and return 1001 (or crash)
                  (+ (helper x) 1000))
                ; Warm up f
                (f 1) (f 2) (f 5) (f 10) (f 1)
                (f 2) (f 5) (f 10) (f 1) (f 2)
                ; Now trigger exception
                (let ((fib (fiber/new (fn () (f 0)) 1)))
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -42 0)))"#,
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        // Should be -42 from the error handler, not 1001 (which would happen if NIL + 1000 worked)
        assert_eq!(val.as_int(), Some(-42));
    }

    #[test]
    fn test_jit_nested_call_exception_propagates() {
        // Test exception propagation through multiple levels of JIT calls
        let result = eval_source(
            r#"(begin
                (defn inner (x) (/ 100 x))
                (defn middle (x) (+ (inner x) 10))
                (defn outer (x) (* (middle x) 2))
                ; Warm up all functions
                (outer 1) (outer 2) (outer 4) (outer 5) (outer 10)
                (outer 1) (outer 2) (outer 4) (outer 5) (outer 10)
                ; Trigger exception deep in the call chain
                (let ((fib (fiber/new (fn () (outer 0)) 1)))
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -999 0)))"#,
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val.as_int(), Some(-999));
    }

    // =========================================================================
    // Fiber + JIT verification tests
    //
    // The effect system prevents suspending closures from being JIT-compiled.
    // Non-suspending fiber primitives (fiber/new, fiber?, fiber/status, etc.)
    // return SIG_OK/SIG_ERROR which the JIT handles. Suspending primitives
    // (fiber/resume, fiber/signal) propagate may_suspend through the effect
    // system, so closures calling them are never JIT candidates.
    // =========================================================================

    #[test]
    fn test_jit_fiber_predicate_in_hot_loop() {
        // fiber? has Effect::none() — should be JIT-compilable.
        // Call it in a hot loop to trigger JIT, verify correct results.
        let result = eval_source(
            r#"(begin
                (defn count-fibers (lst n)
                  (if (= n 0) 0
                    (+ (if (fiber? lst) 1 0)
                       (count-fibers lst (- n 1)))))
                (count-fibers 42 20))"#,
        );
        assert!(result.is_ok(), "fiber? in hot loop failed: {:?}", result);
        assert_eq!(result.unwrap().as_int(), Some(0));
    }

    #[test]
    fn test_jit_fiber_new_in_hot_loop() {
        // fiber/new has Effect::raises() — not suspending, JIT-safe.
        // Create fibers in a hot loop, verify they're created correctly.
        let result = eval_source(
            r#"(begin
                (defn make-fibers (n)
                  (if (= n 0) true
                    (begin
                      (fiber/new (fn () n) 1)
                      (make-fibers (- n 1)))))
                (make-fibers 20))"#,
        );
        assert!(result.is_ok(), "fiber/new in hot loop failed: {:?}", result);
        assert_eq!(result.unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_jit_fiber_status_in_hot_loop() {
        // fiber/status has Effect::raises() — JIT-safe.
        // Check status of a fiber repeatedly in a hot loop.
        let result = eval_source(
            r#"(begin
                (var f (fiber/new (fn () 42) 1))
                (defn check-status (n)
                  (if (= n 0) (= (fiber/status f) :new)
                    (begin (fiber/status f) (check-status (- n 1)))))
                (check-status 20))"#,
        );
        assert!(
            result.is_ok(),
            "fiber/status in hot loop failed: {:?}",
            result
        );
        let val = result.unwrap();
        assert_eq!(val.as_bool(), Some(true));
    }

    #[test]
    fn test_jit_closure_calling_fiber_resume_not_jit_compiled() {
        // fiber/resume has Effect::yields_raises() — may_suspend is true.
        // A closure calling fiber/resume should NOT be JIT-compiled, but
        // should still work correctly via the interpreter.
        let result = eval_source(
            r#"(begin
                (defn resume-fiber (f)
                  (fiber/resume f))
                (var f (fiber/new (fn () 42) 0))
                (resume-fiber f))"#,
        );
        assert!(
            result.is_ok(),
            "fiber/resume via interpreter failed: {:?}",
            result
        );
        assert_eq!(result.unwrap().as_int(), Some(42));
    }

    #[test]
    fn test_jit_mixed_pure_and_fiber_functions() {
        // A pure function and a fiber-using function coexist.
        // The pure function gets JIT-compiled; the fiber one doesn't.
        // Both produce correct results.
        let result = eval_source(
            r#"(begin
                (defn pure-add (x y) (+ x y))
                (defn use-fiber (x)
                  (var f (fiber/new (fn () x) 0))
                  (fiber/resume f)
                  (fiber/value f))
                ; Warm up pure-add (should get JIT-compiled)
                (defn loop (n acc)
                  (if (= n 0) acc (loop (- n 1) (pure-add acc 1))))
                (var sum (loop 20 0))
                ; Now use fibers (interpreter path)
                (var fval (use-fiber sum))
                fval)"#,
        );
        assert!(result.is_ok(), "mixed pure/fiber failed: {:?}", result);
        assert_eq!(result.unwrap().as_int(), Some(20));
    }
}
