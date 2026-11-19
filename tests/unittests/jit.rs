// audited: 2026-11-18
// docs/impl/jit.md
// Programs whose calls pass the JIT's hotness threshold answer as the interpreter would.

mod jit_tests {
    use elle::value::Value;

    /// Evaluate `input` with the stdlib, the graft off, and inspect the result
    /// while its heap is alive. The subject of these tests is a call that turns
    /// hot, and the graft would put a short callee's body in place of the call
    /// (docs/impl/graft.md).
    fn eval_source<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
        let mut rt = elle::runtime::Runtime::new();
        rt.compile().set_graft(elle::config::GraftPolicy::Off);
        let result = {
            let (vm, symbols, cctx) = rt.parts();
            elle::eval_all(input, symbols, vm, cctx, "<test>")
        };
        f(result)
    }

    #[test]
    fn test_jit_triggered_by_hot_loop() {
        // Call a function 20 times, past the threshold of ten calls
        let code = r#"(begin
            (defn add1 (x) (+ x 1))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (add1 acc))))
            (loop 20 0))"#;
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(20)));
    }

    #[test]
    fn test_jit_simple_arithmetic() {
        // Simple arithmetic function called many times
        let code = r#"(begin
            (defn square (x) (* x x))
            (defn sum-squares (n acc)
              (if (= n 0) acc (sum-squares (- n 1) (+ acc (square n)))))
            (sum-squares 15 0))"#;
        // Sum of squares from 1 to 15 = 1240
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(1240)));
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
        // 15 * 5 = 75
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(75)));
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
        // Should return (false true false true false) for (= 5 10), (< 5 10), etc.
        eval_source(code, |r| assert!(r.unwrap().is_pair()));
    }

    #[test]
    fn test_jit_modulo_and_division() {
        // Test modulo and division operations
        let code = r#"(begin
            (defn mod-div-test (x y)
              (+ (/ x y) (rem x y)))
            (defn loop (n acc)
              (if (= n 1) acc (loop (- n 1) (+ acc (mod-div-test n 2)))))
            (loop 15 0))"#;
        // For n from 15 down to 2, each term is (n/2) + (n%2).
        // 15: 7+1=8, 14: 7+0=7, 13: 6+1=7, 12: 6+0=6, 11: 5+1=6, 10: 5+0=5.
        // 9: 4+1=5, 8: 4+0=4, 7: 3+1=4, 6: 3+0=3, 5: 2+1=3, 4: 2+0=2.
        // 3: 1+1=2, 2: 1+0=1.
        // Sum = 8+7+7+6+6+5+5+4+4+3+3+2+2+1 = 63.
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(63)));
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
        // Sum of |n - 8| for n from 1 to 15
        // = |1-8| + |2-8| + ... + |15-8|
        // = 7 + 6 + 5 + 4 + 3 + 2 + 1 + 0 + 1 + 2 + 3 + 4 + 5 + 6 + 7 = 56
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(56)));
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
        // Each call to h adds 4, so 12 * 4 = 48
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(48)));
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
        eval_source(code, |r| assert!(r.unwrap().is_float()));
    }

    #[test]
    fn test_jit_identity_function() {
        // Simple identity function
        let code = r#"(begin
            (defn id (x) x)
            (defn loop (n)
              (if (= n 0) (id 42) (begin (id n) (loop (- n 1)))))
            (loop 15))"#;
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(42)));
    }

    #[test]
    fn test_jit_multiple_args() {
        // Function with multiple arguments
        let code = r#"(begin
            (defn add3 (a b c) (+ a (+ b c)))
            (defn loop (n acc)
              (if (= n 0) acc (loop (- n 1) (add3 acc n 1))))
            (loop 12 0))"#;
        // Each iteration adds n + 1 to acc, so the sum over n from 1 to 12 is
        // 78 + 12 = 90.
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(90)));
    }

    #[test]
    fn test_jit_does_not_break_non_pure() {
        // A function that assigns a captured var answers correctly
        let code = r#"(begin
            (var counter 0)
            (defn inc! ()
              (assign counter (+ counter 1))
              counter)
            (defn loop (n)
              (if (= n 0) counter (begin (inc!) (loop (- n 1)))))
            (loop 15))"#;
        eval_source(code, |r| assert_eq!(r.unwrap(), Value::int(15)));
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
                     (assign results (pair (fib 10) results))
                    (collect (- n 1)))))
            (collect 15))"#;
        eval_source(code, |r| {
            let result = r.unwrap();
            // All results should be fib(10) = 55
            // Verify the list is non-empty and all elements are 55
            assert!(result.is_pair());
            let mut current = result;
            while let Some(pair) = current.as_pair() {
                assert_eq!(pair.first, Value::int(55));
                current = pair.rest;
            }
        });
    }

    #[test]
    fn test_jit_tail_call_basic() {
        // Tail-recursive sum — should work correctly with TCO
        let result = eval_source(
            r#"(begin
            (defn sum-to (n acc)
                (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
            (sum-to 100 0))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap(), Some(5050));
    }

    #[test]
    fn test_jit_tail_call_deep_recursion() {
        // Deep tail recursion that would blow the stack without TCO
        let result = eval_source(
            r#"(begin
            (defn count-down (n)
                (if (= n 0) 0 (count-down (- n 1))))
            (count-down 50000))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap(), Some(0));
    }

    #[test]
    fn test_jit_tail_call_mutual_recursion() {
        // Mutual recursion via tail calls
        let result = eval_source(
            r#"(letrec [is-even (fn (n)
                          (if (= n 0) true (is-odd (- n 1))))
                        is-odd (fn (n)
                          (if (= n 0) false (is-even (- n 1))))]
              (is-even 100))"#,
            |r| r.map(|v| v.as_bool()),
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap(), Some(true));
    }

    #[test]
    fn test_jit_does_not_regress_recursive_workloads() {
        // A self tail call in compiled code is a native loop, and the
        // tail-recursive function answers correctly
        let result = eval_source(
            r#"(begin
            (defn fib-tail (n a b)
                (if (= n 0) a
                    (if (= n 1) b
                        (fib-tail (- n 1) b (+ a b)))))
            (fib-tail 30 0 1))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let val = result.unwrap();
        assert_eq!(val, Some(832040));
    }

    #[test]
    fn test_jit_callee_exception_propagates() {
        // A hot function calls another function that raises. The JIT must
        // propagate the error, not swallow it: when `f` is compiled and calls
        // `helper` with x=0, the division-by-zero error reaches the fiber.
        let result = eval_source(
            r#"(begin
                (defn helper (x) (/ 10 x))
                (defn f (x) (+ (helper x) 1))
                # Call f enough times to make it hot, then call with 0
                (f 1) (f 2) (f 3) (f 4) (f 5)
                (f 6) (f 7) (f 8) (f 9) (f 10)
                (let [fib (fiber/new (fn () (f 0)) 1)]
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -1 0)))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        // Should get -1 from the error handler, not garbage from continuing after exception
        assert_eq!(val, Some(-1));
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
                  # Call helper, then add 1000 to result
                  # If exception propagation is broken, this would execute
                  # with result=NIL and return 1001 (or crash)
                  (+ (helper x) 1000))
                # Warm up f
                (f 1) (f 2) (f 5) (f 10) (f 1)
                (f 2) (f 5) (f 10) (f 1) (f 2)
                # Now trigger exception
                (let [fib (fiber/new (fn () (f 0)) 1)]
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -42 0)))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        // Should be -42 from the error handler, not 1001 (which would happen if NIL + 1000 worked)
        assert_eq!(val, Some(-42));
    }

    #[test]
    fn test_jit_nested_call_exception_propagates() {
        // Test exception propagation through multiple levels of JIT calls
        let result = eval_source(
            r#"(begin
                (defn inner (x) (/ 100 x))
                (defn middle (x) (+ (inner x) 10))
                (defn outer (x) (* (middle x) 2))
                # Warm up all functions
                (outer 1) (outer 2) (outer 4) (outer 5) (outer 10)
                (outer 1) (outer 2) (outer 4) (outer 5) (outer 10)
                # Trigger exception deep in the call chain
                (let [fib (fiber/new (fn () (outer 0)) 1)]
                  (fiber/resume fib)
                  (if (= (fiber/bits fib) 1) -999 0)))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val, Some(-999));
    }

    // =========================================================================
    // Fiber + JIT verification tests
    //
    // A function's signal does not decide whether the JIT compiles it. A
    // signal leaves compiled code through the yield protocol, so a hot
    // function that calls a fiber primitive answers as the interpreter would.
    // =========================================================================

    #[test]
    fn test_jit_fiber_predicate_in_hot_loop() {
        // Call fiber? in a hot loop and verify the results.
        let result = eval_source(
            r#"(begin
                (defn count-fibers (lst n)
                  (if (= n 0) 0
                    (+ (if (fiber? lst) 1 0)
                       (count-fibers lst (- n 1)))))
                (count-fibers 42 20))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok(), "fiber? in hot loop failed: {:?}", result);
        assert_eq!(result.unwrap(), Some(0));
    }

    #[test]
    fn test_jit_fiber_new_in_hot_loop() {
        // Create fibers in a hot loop, verify they're created correctly.
        let result = eval_source(
            r#"(begin
                (defn make-fibers (n)
                  (if (= n 0) true
                    (begin
                      (fiber/new (fn () n) 1)
                      (make-fibers (- n 1)))))
                (make-fibers 20))"#,
            |r| r.map(|v| v.as_bool()),
        );
        assert!(result.is_ok(), "fiber/new in hot loop failed: {:?}", result);
        assert_eq!(result.unwrap(), Some(true));
    }

    #[test]
    fn test_jit_fiber_status_in_hot_loop() {
        // Check status of a fiber repeatedly in a hot loop.
        let result = eval_source(
            r#"(begin
                (var f (fiber/new (fn () 42) 1))
                (defn check-status (n)
                  (if (= n 0) (= (fiber/status f) :new)
                    (begin (fiber/status f) (check-status (- n 1)))))
                (check-status 20))"#,
            |r| r.map(|v| v.as_bool()),
        );
        assert!(
            result.is_ok(),
            "fiber/status in hot loop failed: {:?}",
            result
        );
        let val = result.unwrap();
        assert_eq!(val, Some(true));
    }

    #[test]
    fn test_jit_closure_calling_fiber_resume() {
        // A closure calling fiber/resume answers the fiber's value.
        let result = eval_source(
            r#"(begin
                (defn resume-fiber (f)
                  (fiber/resume f))
                (var f (fiber/new (fn () 42) 0))
                (resume-fiber f))"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(
            result.is_ok(),
            "fiber/resume via interpreter failed: {:?}",
            result
        );
        assert_eq!(result.unwrap(), Some(42));
    }

    #[test]
    fn test_jit_mixed_pure_and_fiber_functions() {
        // A hot silent function and a function that resumes a fiber coexist,
        // and both produce correct results.
        let result = eval_source(
            r#"(begin
                (defn pure-add (x y) (+ x y))
                (defn use-fiber (x)
                  (var f (fiber/new (fn () x) 0))
                  (fiber/resume f)
                  (fiber/value f))
                # Warm up pure-add (should get JIT-compiled)
                (defn loop (n acc)
                  (if (= n 0) acc (loop (- n 1) (pure-add acc 1))))
                (var sum (loop 20 0))
                # Now use fibers (interpreter path)
                (var fval (use-fiber sum))
                fval)"#,
            |r| r.map(|v| v.as_int()),
        );
        assert!(result.is_ok(), "mixed pure/fiber failed: {:?}", result);
        assert_eq!(result.unwrap(), Some(20));
    }
}
