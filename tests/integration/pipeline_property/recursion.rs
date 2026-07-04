use super::*;

// ============================================================================
// Recursion Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn recursive_factorial(n in 0u8..8) {
        let expr = format!(
            "(letrec [fact (fn (n) (if (<= n 1) 1 (* n (fact (- n 1)))))]
               (fact {}))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (1..=n as i64).product();
        let expected = if expected == 0 { 1 } else { expected };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn recursive_sum(n in 0u8..20) {
        let expr = format!(
            "(letrec [sum-to (fn (n) (if (<= n 0) 0 (+ n (sum-to (- n 1)))))]
               (sum-to {}))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (0..=n as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn recursive_length(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");
        let expr = format!(
            "(letrec [my-length (fn (lst) (if (empty? lst) 0 (+ 1 (my-length (rest lst)))))]
               (my-length (list {})))",
            list_str
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn tail_recursive_sum(n in 0u8..50) {
        let expr = format!(
            "(letrec [sum-iter (fn (n acc) (if (<= n 0) acc (sum-iter (- n 1) (+ acc n))))]
               (sum-iter {} 0))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (0..=n as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn mutual_recursion_even_odd(n in 0u8..20) {
        let expr = format!(
            "(letrec [is-even (fn (n) (if (= n 0) true (is-odd (- n 1)))) is-odd (fn (n) (if (= n 0) false (is-even (- n 1))))]
               (is-even {}))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(n % 2 == 0));
    }
}

// ============================================================================
// Function as Data Properties
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(20))]

    #[test]
    fn store_function_in_list(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(let [fns (list (fn (x) (+ x 1)) (fn (x) (* x 2)))]
               (+ ((first fns) {}) ((first (rest fns)) {})))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a + 1) + (b * 2)));
    }

    #[test]
    fn function_returning_function_returning_value(a in -30i64..30, b in -30i64..30) {
        // Test a function that returns a function that returns a value
        let expr = format!(
            "(let [f (fn (x) (fn (y) (+ x y)))]
               ((f {}) {}))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Higher-Order Function Properties (map, filter, fold)
// ============================================================================
