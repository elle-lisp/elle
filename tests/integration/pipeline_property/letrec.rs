use super::*;

// ============================================================================
// Nested Define and Letrec Patterns
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn letrec_mutual_recursion_in_lambda(n in 0u8..20) {
        // Mutual recursion inside a lambda using letrec
        let expr = format!(
            "(let [check (fn ()
                            (letrec [is-even (fn (x) (if (= x 0) true (is-odd (- x 1)))) is-odd (fn (x) (if (= x 0) false (is-even (- x 1))))]
                              (is-even {})))]
               (check))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "letrec mutual recursion in lambda failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(n % 2 == 0));
    }

    #[test]
    fn nested_letrec_different_scopes(a in 1i64..10, b in 1i64..10) {
        // Nested letrec blocks with independent bindings
        let expr = format!(
            "(letrec [outer (fn (x)
                              (letrec [inner (fn (y) (* x y))]
                                (inner {})))]
               (outer {}))",
            b, a
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "nested letrec failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * b));
    }

    #[test]
    fn letrec_with_captured_outer_var(outer in 1i64..20, n in 0u8..10) {
        // letrec where inner functions capture outer scope
        let expr = format!(
            "(let [base {}]
               (letrec [add-base (fn (x) (+ base x)) double-add (fn (x) (add-base (add-base x)))]
                 (double-add {})))",
            outer, n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "letrec with captured outer failed: {:?}", result);
        // double-add(n) = add-base(add-base(n)) = (n + base) + base = n + 2*base
        prop_assert_eq!(result.unwrap(), Value::int(n as i64 + 2 * outer));
    }

    #[test]
    fn self_recursive_with_local_define(n in 1u8..10) {
        // Self-recursion with local define (not mutual - this should work)
        let expr = format!(
            "(let [compute (fn (x)
                              (begin
                                (def helper (fn (n acc)
                                  (if (= n 0) acc (helper (- n 1) (+ acc n)))))
                                (helper x 0)))]
               (compute {}))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "self-recursive with local define failed: {:?}", result);
        let expected: i64 = (1..=n as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn counter_in_letrec(increments in 1usize..5) {
        // Counter pattern using letrec for mutual access
        let mut calls = String::new();
        for _ in 0..increments {
            calls.push_str("(inc) ");
        }
        let expr = format!(
            "(let [@n 0]
               (letrec [inc (fn () (begin (assign n (+ n 1)) n)) get (fn () n)]
                 (begin {} (get))))",
            calls
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "counter in letrec failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(increments as i64));
    }

    #[test]
    fn three_way_mutual_letrec(n in 0u8..15) {
        // Three mutually recursive functions
        let h = elle::primitives::ctx::TestHeap::new();
        let expr = format!(
            "(letrec [f (fn (x) (if (= x 0) \"f\" (g (- x 1)))) g (fn (x) (if (= x 0) \"g\" (h (- x 1)))) h (fn (x) (if (= x 0) \"h\" (f (- x 1))))]
               (f {}))",
            n
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "three-way mutual letrec failed: {:?}", result);
        let expected = match n % 3 {
            0 => "f",
            1 => "g",
            2 => "h",
            _ => unreachable!(),
        };
        prop_assert_eq!(result.unwrap(), h.ctx().string(expected));
    }

    #[test]
    fn letrec_with_higher_order(a in 1i64..10, b in 1i64..10, _c in 1i64..10) {
        // letrec where functions take other functions as arguments
        let expr = format!(
            "(letrec [apply-twice (fn (f x) (f (f x))) add-one (fn (x) (+ x 1)) double (fn (x) (* x 2))]
               (+ (apply-twice add-one {})
                  (apply-twice double {})))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "letrec with higher order failed: {:?}", result);
        // apply-twice add-one a = a + 2
        // apply-twice double b = b * 4
        prop_assert_eq!(result.unwrap(), Value::int((a + 2) + (b * 4)));
    }

    #[test]
    fn nested_lambda_with_define_no_forward_ref(a in 1i64..20, b in 1i64..20) {
        // Local define without forward references should work
        let expr = format!(
            "(let [outer (fn (x)
                            (begin
                              (var local (* x 2))
                              (fn (y) (+ local y))))]
               ((outer {}) {}))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "nested lambda with define failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * 2 + b));
    }

    #[test]
    fn sequential_defines_no_mutual(a in 1i64..10, b in 1i64..10) {
        // Sequential defines where second uses first (not mutual)
        let expr = format!(
            "(let [compute (fn ()
                              (begin
                                (var x {})
                                (var y (+ x {}))
                                (var z (* x y))
                                z))]
               (compute))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "sequential defines failed: {:?}", result);
        let x = a;
        let y = a + b;
        let z = x * y;
        prop_assert_eq!(result.unwrap(), Value::int(z));
    }
}
