use super::*;

// ============================================================================
// Lambda / Closure Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn lambda_identity(a in -1000i64..1000) {
        let expr = format!("((fn (x) x) {})", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn lambda_constant(a in -100i64..100, b in -100i64..100) {
        let expr = format!("((fn (x) {}) {})", b, a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn closure_captures_value(captured in -100i64..100, arg in -100i64..100) {
        let expr = format!(
            "(let [y {}] ((fn (x) (+ x y)) {}))",
            captured, arg
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(arg + captured));
    }

    #[test]
    fn lambda_multiple_args(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!("((fn (x y z) (+ x (+ y z))) {} {} {})", a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }
}

// ============================================================================
// List Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn list_first_returns_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(first (list {} {}))", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn list_length_correct(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let expr = format!("(length (list {}))", elements.join(" "));
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn cons_then_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(first (pair {} {}))", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cons_then_rest(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(rest (pair {} {}))", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }
}

// ============================================================================
// Boolean Logic Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    #[test]
    fn not_involution(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(not (not {}))", bool_str);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(b));
    }

    #[test]
    fn and_with_false_is_false(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(and {} false)", bool_str);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(false));
    }

    #[test]
    fn or_with_true_is_true(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(or {} true)", bool_str);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn de_morgan_and(a in prop::bool::ANY, b in prop::bool::ANY) {
        // not(a and b) == (not a) or (not b)
        let a_str = if a { "true" } else { "false" };
        let b_str = if b { "true" } else { "false" };

        let expr1 = format!("(not (and {} {}))", a_str, b_str);
        let expr2 = format!("(or (not {}) (not {}))", a_str, b_str);

        let r1 = eval_reuse(&expr1);
        let r2 = eval_reuse(&expr2);

        prop_assert!(r1.is_ok());
        prop_assert!(r2.is_ok());
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }
}

// ============================================================================
// Match Expression Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn match_literal_exact(a in -100i64..100) {
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!("(match {} {} \"hit\" _ \"miss\")", a, a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("hit"));
    }

    #[test]
    fn match_wildcard_fallback(a in -100i64..100) {
        // Match against a different literal, should fall to wildcard
        let h = elle::primitives::ctx::TestHeap::new();
        let other = a.wrapping_add(1);
        let expr = format!("(match {} {} \"hit\" _ \"miss\")", a, other);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("miss"));
    }

    #[test]
    fn match_with_computed_body(a in -50i64..50, b in -50i64..50) {
        let expr = format!("(match {} {} (+ {} {}) _ 0)", a, a, a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Array Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn array_length_correct(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let expr = if elements.is_empty() {
            "(length @[])".to_string()
        } else {
            format!("(length @[{}])", elements.join(" "))
        };
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn array_ref_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(get @[{} {}] 0)", a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }
}

// ============================================================================
// Match Expression Extended Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn match_multiple_literals_first_matches(a in -100i64..100) {
        // First of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!("(match {} {} \"first\" {} \"second\" {} \"third\" _ \"default\")", a, a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("first"));
    }

    #[test]
    fn match_multiple_literals_middle_matches(a in -100i64..100) {
        // Middle of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!("(match {} {} \"first\" {} \"second\" {} \"third\" _ \"default\")", b, a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("second"));
    }

    #[test]
    fn match_multiple_literals_last_matches(a in -100i64..100) {
        // Last of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!("(match {} {} \"first\" {} \"second\" {} \"third\" _ \"default\")", c, a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("third"));
    }

    #[test]
    fn match_with_arithmetic_in_body(a in -50i64..50, b in -50i64..50) {
        // Match with computation in body (the bug we just fixed)
        let expr = format!("(match {} {} (+ {} {}) _ 0)", a, a, a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    #[test]
    fn match_nil_pattern(a in -100i64..100) {
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!("(match nil nil \"is-nil\" {} \"is-num\" _ \"other\")", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), h.ctx().string("is-nil"));
    }
}

// ============================================================================
// Each/For Loop Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn each_iterates_all_elements(len in 1usize..5) {
        // Create a list and count iterations using a counter
        let elements: Vec<String> = (1..=len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");

        // Sum all elements
        let expr = format!(
            "(let [@sum 0] (begin (each x (list {}) (assign sum (+ sum x))) sum))",
            list_str
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (1..=len as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn each_empty_list_no_iteration(a in -100i64..100) {
        // Each over empty list should not execute body, return nil
        let expr = format!("(let [@x {}] (begin (each y (list) (assign x 999)) x))", a);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a)); // x unchanged
    }
}
