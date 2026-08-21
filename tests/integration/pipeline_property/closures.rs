use super::*;

// ============================================================================
// Closure Mutation Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn closure_mutation_persists(_start in 0i64..100, increments in 1usize..5) {
        // Counter closure that mutates captured variable
        let mut expr = String::from(
            "(let [counter (let [@n 0] (fn () (begin (assign n (+ n 1)) n)))]"
        );

        // Call counter multiple times
        for _ in 0..increments {
            expr.push_str(" (counter)");
        }
        expr.push(')');

        let result = eval_reuse(&expr);
        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(increments as i64));
    }

    #[test]
    fn independent_closures_have_separate_state(a in 1i64..50, b in 1i64..50) {
        // Two independent closures with separate captured state
        let expr = format!(
            "(let [c1 (let [@n {}] (fn () (begin (assign n (+ n 1)) n))) c2 (let [@m {}] (fn () (begin (assign m (+ m 1)) m)))]
                (begin (c1) (c1) (c2) (list (c1) (c2))))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        // c1 called 3 times: a+1, a+2, a+3
        // c2 called 2 times: b+1, b+2
        // Result should be list of (a+3, b+2)
    }

    #[test]
    fn closure_captures_and_mutates(start in 0i64..50, increments in 1usize..5) {
        // Basic closure that captures and mutates a variable
        let mut calls = String::new();
        for _ in 0..increments {
            calls.push_str("(inc) ");
        }
        let expr = format!(
            "(let [@n {}]
               (let [inc (fn () (begin (assign n (+ n 1)) n))]
                 (begin {})))",
            start, calls
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(start + increments as i64));
    }

    #[test]
    fn counter_factory_single(start in 0i64..100) {
        // Single counter from factory
        let expr = format!(
            "(let [make-counter (fn (@n) (fn () (begin (assign n (+ n 1)) n)))]
               (let [c (make-counter {})]
                 (begin (c) (c) (c))))",
            start
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(start + 3));
    }

    #[test]
    fn counter_factory_independence(a in 0i64..50, b in 100i64..150) {
        // Two counters from same factory must be independent
        // This is the critical test that catches shared-state bugs
        // c1 called twice: a+1, a+2
        // c2 called once: b+1
        // Final call: c1 at a+3, c2 at b+2
        let expr = format!(
            "(let [make-counter (fn (@n) (fn () (begin (assign n (+ n 1)) n)))]
               (let [c1 (make-counter {}) c2 (make-counter {})]
                 (begin
                   (c1) (c1)
                   (c2)
                   (+ (c1) (c2)))))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a + 3) + (b + 2)));
    }

    #[test]
    fn closure_mutates_outer_scope(outer in 0i64..100, delta in 1i64..10) {
        let expr = format!(
            "(let [@x {}]
               (let [add (fn () (assign x (+ x {})))]
                 (begin (add) (add) x)))",
            outer, delta
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer + 2 * delta));
    }

    #[test]
    fn multiple_closures_share_state(init in 0i64..50) {
        // Multiple closures over same variable should share state
        let expr = format!(
            "(let [@n {}]
               (let [inc (fn () (begin (assign n (+ n 1)) n)) dec (fn () (begin (assign n (- n 1)) n)) get (fn () n)]
                 (begin (inc) (inc) (dec) (get))))",
            init
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(init + 1)); // +2 -1 = +1
    }

    #[test]
    fn nested_closure_mutation(a in 0i64..30, b in 0i64..30) {
        // Nested closures, inner mutates outer's captured var
        let expr = format!(
            "(let [@x {}]
               (let [outer (fn (y)
                              (begin (assign x (+ x y)) x))]
                 (begin (outer {}) (outer {}))))",
            a, b, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + b));
    }

    #[test]
    fn closure_over_parameter(param in 0i64..50, delta in 1i64..10) {
        // Closure captures function parameter and mutates it
        let expr = format!(
            "(let [make-mutator (fn (@n)
                                   (fn () (begin (assign n (+ n {})) n)))]
               (let [m (make-mutator {})]
                 (begin (m) (m) (m))))",
            delta, param
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(param + 3 * delta));
    }

    #[test]
    fn accumulator_pattern(init in 0i64..20, values in prop::collection::vec(1i64..10, 1..5)) {
        // Accumulator pattern: closure that adds to running total
        let mut calls = String::new();
        for v in &values {
            calls.push_str(&format!("(add {}) ", v));
        }
        let expr = format!(
            "(let [@total {}]
               (let [add (fn (x) (begin (assign total (+ total x)) total))]
                 (begin {})))",
            init, calls
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = init + values.iter().sum::<i64>();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }
}
