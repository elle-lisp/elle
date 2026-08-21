use super::*;

// ============================================================================
// Arithmetic Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    #[test]
    fn addition_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let expr1 = format!("(+ {} {})", a, b);
        let expr2 = format!("(+ {} {})", b, a);

        let r1 = eval_reuse(&expr1);
        let r2 = eval_reuse(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn addition_associative(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr1 = format!("(+ (+ {} {}) {})", a, b, c);
        let expr2 = format!("(+ {} (+ {} {}))", a, b, c);

        let r1 = eval_reuse(&expr1);
        let r2 = eval_reuse(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn addition_identity(a in -1000i64..1000) {
        let expr = format!("(+ {} 0)", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn multiplication_commutative(a in -100i64..100, b in -100i64..100) {
        let expr1 = format!("(* {} {})", a, b);
        let expr2 = format!("(* {} {})", b, a);

        let r1 = eval_reuse(&expr1);
        let r2 = eval_reuse(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn multiplication_identity(a in -1000i64..1000) {
        let expr = format!("(* {} 1)", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn subtraction_inverse_of_addition(a in -500i64..500, b in -500i64..500) {
        let expr = format!("(- (+ {} {}) {})", a, b, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn division_inverse_of_multiplication(a in -100i64..100, b in 1i64..100) {
        let expr = format!("(/ (* {} {}) {})", a, b, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }
}

// ============================================================================
// Comparison Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    #[test]
    fn equality_reflexive(a in -1000i64..1000) {
        let expr = format!("(= {} {})", a, a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn equality_symmetric(a in -100i64..100, b in -100i64..100) {
        let expr1 = format!("(= {} {})", a, b);
        let expr2 = format!("(= {} {})", b, a);

        let r1 = eval_reuse(&expr1);
        let r2 = eval_reuse(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn less_than_irreflexive(a in -1000i64..1000) {
        let expr = format!("(< {} {})", a, a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(false));
    }

    #[test]
    fn less_than_asymmetric(a in -100i64..100, b in -100i64..100) {
        if a < b {
            let expr1 = format!("(< {} {})", a, b);
            let expr2 = format!("(< {} {})", b, a);

            let r1 = eval_reuse(&expr1);
            let r2 = eval_reuse(&expr2);

            prop_assert!(r1.is_ok());
            prop_assert!(r2.is_ok());
            prop_assert_eq!(r1.unwrap(), Value::bool(true));
            prop_assert_eq!(r2.unwrap(), Value::bool(false));
        }
    }
}

// ============================================================================
// Conditional Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    #[test]
    fn if_true_returns_then(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(if true {} {})", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn if_false_returns_else(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(if false {} {})", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn if_with_computed_condition(a in -100i64..100, b in -100i64..100) {
        // (if (< a b) a b) should return the smaller value
        let expr = format!("(if (< {} {}) {} {})", a, b, a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if a < b { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn nested_if_consistency(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        // Nested if should work correctly
        let expr = format!(
            "(if (< {} {}) (if (< {} {}) {} {}) {})",
            a, b, a, c, a, c, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
    }
}

// ============================================================================
// Let Binding Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    #[test]
    fn let_binds_value(a in -1000i64..1000) {
        let expr = format!("(let [x {}] x)", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn let_shadows_outer(outer in -100i64..100, inner in -100i64..100) {
        let expr = format!("(let [x {}] (let [x {}] x))", outer, inner);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(inner));
    }

    #[test]
    fn let_outer_unchanged_after_inner(outer in -100i64..100, inner in -100i64..100) {
        // After inner let exits, outer binding should be accessible
        let expr = format!(
            "(let [x {}] (begin (let [x {}] x) x))",
            outer, inner
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer));
    }

    #[test]
    fn let_multiple_bindings(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(let [x {} y {}] (+ x y))", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}
