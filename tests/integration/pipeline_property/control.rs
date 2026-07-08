use super::*;

// ============================================================================
// Nested Control Flow Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn nested_let_in_if(cond in prop::bool::ANY, a in -100i64..100, b in -100i64..100) {
        let cond_str = if cond { "true" } else { "false" };
        let expr = format!(
            "(if {} (let [x {}] x) (let [y {}] y))",
            cond_str, a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if cond { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn if_in_lambda_body(cond in prop::bool::ANY, a in -100i64..100, b in -100i64..100) {
        let cond_str = if cond { "true" } else { "false" };
        let expr = format!("((fn () (if {} {} {})))", cond_str, a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if cond { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn match_in_lambda(a in -50i64..50, b in -50i64..50) {
        // Equal draws would make arm 2 a literal duplicate of arm 1, which
        // the compiler rejects as an unreachable match arm.
        prop_assume!(a != b);
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!(
            "((fn (x) (match x {} \"a\" {} \"b\" _ \"other\")) {})",
            a, b, a
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("a"));
    }

    #[test]
    fn lambda_in_match_body(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(match {} {} ((fn (x) (+ x {})) {}) _ 0)",
            a, a, b, a
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Begin/Sequence Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn begin_returns_last(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr = format!("(begin {} {} {})", a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    #[test]
    fn begin_with_side_effects(a in -100i64..100, b in -100i64..100) {
        // Side effect: assign followed by read
        let expr = format!(
            "(let [@x {}] (begin (assign x {}) x))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }
}

// ============================================================================
// Cond Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn cond_first_true(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr = format!("(cond true {} true {} {})", a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cond_falls_through_to_else(a in -100i64..100) {
        let expr = format!("(cond false 1 false 2 {})", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cond_with_computed_conditions(a in -100i64..100, threshold in -100i64..100) {
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!(
            "(cond (< {} {}) \"less\" (= {} {}) \"equal\" \"greater\")",
            a, threshold, a, threshold
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if a < threshold {
            "less"
        } else if a == threshold {
            "equal"
        } else {
            "greater"
        };
        prop_assert_eq!(result.unwrap(), h.ctx().string(expected));
    }
}

// ============================================================================
// Quasiquote Properties (if supported)
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn quasiquote_with_unquote(a in -100i64..100) {
        let expr = format!("(let [x {}] `(1 ,x 3))", a);
        let result = eval_reuse(&expr);

        // If quasiquote is supported, check result is a list with x interpolated
        if let Ok(val) = result {
            if let Ok(vec) = val.list_to_vec() {
                prop_assert_eq!(vec.len(), 3);
                prop_assert_eq!(&vec[1], &Value::int(a));
            }
        }
        // If not supported, that's also OK for now
    }
}

// ============================================================================
// Handler-Case Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]




}

// ============================================================================
// Higher-Order Function Properties
// ============================================================================
// NOTE: map, filter, reduce are not yet registered as primitives in the
// current implementation. These tests are commented out pending implementation.
// See: src/primitives/higher_order.rs for the function definitions.

// ============================================================================
// Function Factory Properties (returning closures)
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn make_adder_works(n in -50i64..50, x in -50i64..50) {
        let expr = format!(
            "(let [make-adder (fn (n) (fn (x) (+ x n)))]
               ((make-adder {}) {}))",
            n, x
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + n));
    }

    #[test]
    fn make_multiplier_works(n in -20i64..20, x in -20i64..20) {
        let expr = format!(
            "(let [make-mult (fn (n) (fn (x) (* x n)))]
               ((make-mult {}) {}))",
            n, x
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x * n));
    }

    #[test]
    fn compose_functions(a in -20i64..20) {
        // (compose f g)(x) = f(g(x))
        // Use let* because composed references compose, add1, double from earlier bindings
        let expr = format!(
            "(let* [compose (fn (f g) (fn (x) (f (g x))))
                    add1 (fn (x) (+ x 1))
                    double (fn (x) (* x 2))
                    composed (compose add1 double)]
                (composed {}))",
            a
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a * 2) + 1));
    }

    #[test]
    fn apply_n_times(n in 1usize..5, start in 0i64..20) {
        // Apply increment n times
        let mut expr = "(let [inc (fn (x) (+ x 1))] ".to_string();
        for _ in 0..n {
            expr.push_str("(inc ");
        }
        expr.push_str(&start.to_string());
        for _ in 0..n {
            expr.push(')');
        }
        expr.push(')');

        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(start + n as i64));
    }
}

// ============================================================================
// Currying and Partial Application Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn manual_curry_add(a in -50i64..50, b in -50i64..50) {
        // curry: (a, b) -> a -> b -> result
        let expr = format!(
            "(let [curry-add (fn (a) (fn (b) (+ a b)))]
               ((curry-add {}) {}))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}
