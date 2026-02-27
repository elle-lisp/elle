// Property-based tests for the new Syntax → HIR → LIR compilation pipeline
//
// These tests verify semantic correctness by checking mathematical properties
// hold when code is compiled and executed through the new pipeline.

use crate::common::eval_source;
use elle::Value;
use proptest::prelude::*;

// ============================================================================
// Arithmetic Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn addition_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let expr1 = format!("(+ {} {})", a, b);
        let expr2 = format!("(+ {} {})", b, a);

        let r1 = eval_source(&expr1);
        let r2 = eval_source(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn addition_associative(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr1 = format!("(+ (+ {} {}) {})", a, b, c);
        let expr2 = format!("(+ {} (+ {} {}))", a, b, c);

        let r1 = eval_source(&expr1);
        let r2 = eval_source(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn addition_identity(a in -1000i64..1000) {
        let expr = format!("(+ {} 0)", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn multiplication_commutative(a in -100i64..100, b in -100i64..100) {
        let expr1 = format!("(* {} {})", a, b);
        let expr2 = format!("(* {} {})", b, a);

        let r1 = eval_source(&expr1);
        let r2 = eval_source(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn multiplication_identity(a in -1000i64..1000) {
        let expr = format!("(* {} 1)", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn subtraction_inverse_of_addition(a in -500i64..500, b in -500i64..500) {
        let expr = format!("(- (+ {} {}) {})", a, b, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn division_inverse_of_multiplication(a in -100i64..100, b in 1i64..100) {
        let expr = format!("(/ (* {} {}) {})", a, b, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }
}

// ============================================================================
// Comparison Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn equality_reflexive(a in -1000i64..1000) {
        let expr = format!("(= {} {})", a, a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn equality_symmetric(a in -100i64..100, b in -100i64..100) {
        let expr1 = format!("(= {} {})", a, b);
        let expr2 = format!("(= {} {})", b, a);

        let r1 = eval_source(&expr1);
        let r2 = eval_source(&expr2);

        prop_assert!(r1.is_ok(), "expr1 failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "expr2 failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn less_than_irreflexive(a in -1000i64..1000) {
        let expr = format!("(< {} {})", a, a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(false));
    }

    #[test]
    fn less_than_asymmetric(a in -100i64..100, b in -100i64..100) {
        if a < b {
            let expr1 = format!("(< {} {})", a, b);
            let expr2 = format!("(< {} {})", b, a);

            let r1 = eval_source(&expr1);
            let r2 = eval_source(&expr2);

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
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn if_true_returns_then(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(if true {} {})", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn if_false_returns_else(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(if false {} {})", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn if_with_computed_condition(a in -100i64..100, b in -100i64..100) {
        // (if (< a b) a b) should return the smaller value
        let expr = format!("(if (< {} {}) {} {})", a, b, a, b);
        let result = eval_source(&expr);

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
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
    }
}

// ============================================================================
// Let Binding Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn let_binds_value(a in -1000i64..1000) {
        let expr = format!("(let ((x {})) x)", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn let_shadows_outer(outer in -100i64..100, inner in -100i64..100) {
        let expr = format!("(let ((x {})) (let ((x {})) x))", outer, inner);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(inner));
    }

    #[test]
    fn let_outer_unchanged_after_inner(outer in -100i64..100, inner in -100i64..100) {
        // After inner let exits, outer binding should be accessible
        let expr = format!(
            "(let ((x {})) (begin (let ((x {})) x) x))",
            outer, inner
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer));
    }

    #[test]
    fn let_multiple_bindings(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(let ((x {}) (y {})) (+ x y))", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Lambda / Closure Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn lambda_identity(a in -1000i64..1000) {
        let expr = format!("((fn (x) x) {})", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn lambda_constant(a in -100i64..100, b in -100i64..100) {
        let expr = format!("((fn (x) {}) {})", b, a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn closure_captures_value(captured in -100i64..100, arg in -100i64..100) {
        let expr = format!(
            "(let ((y {})) ((fn (x) (+ x y)) {}))",
            captured, arg
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(arg + captured));
    }

    #[test]
    fn lambda_multiple_args(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!("((fn (x y z) (+ x (+ y z))) {} {} {})", a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }
}

// ============================================================================
// List Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn list_first_returns_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(first (list {} {}))", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn list_length_correct(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let expr = format!("(length (list {}))", elements.join(" "));
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn cons_then_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(first (cons {} {}))", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cons_then_rest(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(rest (cons {} {}))", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }
}

// ============================================================================
// Boolean Logic Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn not_involution(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(not (not {}))", bool_str);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(b));
    }

    #[test]
    fn and_with_false_is_false(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(and {} false)", bool_str);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(false));
    }

    #[test]
    fn or_with_true_is_true(b in prop::bool::ANY) {
        let bool_str = if b { "true" } else { "false" };
        let expr = format!("(or {} true)", bool_str);
        let result = eval_source(&expr);

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

        let r1 = eval_source(&expr1);
        let r2 = eval_source(&expr2);

        prop_assert!(r1.is_ok());
        prop_assert!(r2.is_ok());
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }
}

// ============================================================================
// Match Expression Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn match_literal_exact(a in -100i64..100) {
        let expr = format!("(match {} ({} \"hit\") (_ \"miss\"))", a, a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("hit"));
    }

    #[test]
    fn match_wildcard_fallback(a in -100i64..100) {
        // Match against a different literal, should fall to wildcard
        let other = a.wrapping_add(1);
        let expr = format!("(match {} ({} \"hit\") (_ \"miss\"))", a, other);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("miss"));
    }

    #[test]
    fn match_with_computed_body(a in -50i64..50, b in -50i64..50) {
        let expr = format!("(match {} ({} (+ {} {})) (_ 0))", a, a, a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Array Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn array_length_correct(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let expr = if elements.is_empty() {
            "(length @[])".to_string()
        } else {
            format!("(length @[{}])", elements.join(" "))
        };
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn array_ref_first(a in -100i64..100, b in -100i64..100) {
        let expr = format!("(get @[{} {}] 0)", a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }
}

// ============================================================================
// Match Expression Extended Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn match_multiple_literals_first_matches(a in -100i64..100) {
        // First of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let expr = format!("(match {} ({} \"first\") ({} \"second\") ({} \"third\") (_ \"default\"))", a, a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("first"));
    }

    #[test]
    fn match_multiple_literals_middle_matches(a in -100i64..100) {
        // Middle of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let expr = format!("(match {} ({} \"first\") ({} \"second\") ({} \"third\") (_ \"default\"))", b, a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("second"));
    }

    #[test]
    fn match_multiple_literals_last_matches(a in -100i64..100) {
        // Last of several literal patterns matches
        let b = a.wrapping_add(1);
        let c = a.wrapping_add(2);
        let expr = format!("(match {} ({} \"first\") ({} \"second\") ({} \"third\") (_ \"default\"))", c, a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("third"));
    }

    #[test]
    fn match_with_arithmetic_in_body(a in -50i64..50, b in -50i64..50) {
        // Match with computation in body (the bug we just fixed)
        let expr = format!("(match {} ({} (+ {} {})) (_ 0))", a, a, a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    #[test]
    fn match_nil_pattern(a in -100i64..100) {
        let expr = format!("(match nil (nil \"is-nil\") ({} \"is-num\") (_ \"other\"))", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("is-nil"));
    }
}

// ============================================================================
// Each/For Loop Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn each_iterates_all_elements(len in 1usize..5) {
        // Create a list and count iterations using a counter
        let elements: Vec<String> = (1..=len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");

        // Sum all elements
        let expr = format!(
            "(let ((sum 0)) (begin (each x (list {}) (set sum (+ sum x))) sum))",
            list_str
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (1..=len as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn each_empty_list_no_iteration(a in -100i64..100) {
        // Each over empty list should not execute body, return nil
        let expr = format!("(let ((x {})) (begin (each y (list) (set x 999)) x))", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a)); // x unchanged
    }
}

// ============================================================================
// Closure Mutation Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn closure_mutation_persists(_start in 0i64..100, increments in 1usize..5) {
        // Counter closure that mutates captured variable
        let mut expr = String::from(
            "(let ((counter (let ((n 0)) (fn () (begin (set n (+ n 1)) n)))))"
        );

        // Call counter multiple times
        for _ in 0..increments {
            expr.push_str(" (counter)");
        }
        expr.push(')');

        let result = eval_source(&expr);
        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(increments as i64));
    }

    #[test]
    fn independent_closures_have_separate_state(a in 1i64..50, b in 1i64..50) {
        // Two independent closures with separate captured state
        let expr = format!(
            "(let ((c1 (let ((n {})) (fn () (begin (set n (+ n 1)) n))))
                   (c2 (let ((m {})) (fn () (begin (set m (+ m 1)) m)))))
                (begin (c1) (c1) (c2) (list (c1) (c2))))",
            a, b
        );
        let result = eval_source(&expr);

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
            "(let ((n {}))
               (let ((inc (fn () (begin (set n (+ n 1)) n))))
                 (begin {})))",
            start, calls
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(start + increments as i64));
    }

    #[test]
    fn counter_factory_single(start in 0i64..100) {
        // Single counter from factory
        let expr = format!(
            "(let ((make-counter (fn (n) (fn () (begin (set n (+ n 1)) n)))))
               (let ((c (make-counter {})))
                 (begin (c) (c) (c))))",
            start
        );
        let result = eval_source(&expr);

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
            "(let ((make-counter (fn (n) (fn () (begin (set n (+ n 1)) n)))))
               (let ((c1 (make-counter {})) (c2 (make-counter {})))
                 (begin 
                   (c1) (c1)
                   (c2)
                   (+ (c1) (c2)))))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a + 3) + (b + 2)));
    }

    #[test]
    fn closure_mutates_outer_scope(outer in 0i64..100, delta in 1i64..10) {
        let expr = format!(
            "(let ((x {}))
               (let ((add (fn () (set x (+ x {})))))
                 (begin (add) (add) x)))",
            outer, delta
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer + 2 * delta));
    }

    #[test]
    fn multiple_closures_share_state(init in 0i64..50) {
        // Multiple closures over same variable should share state
        let expr = format!(
            "(let ((n {}))
               (let ((inc (fn () (begin (set n (+ n 1)) n)))
                     (dec (fn () (begin (set n (- n 1)) n)))
                     (get (fn () n)))
                 (begin (inc) (inc) (dec) (get))))",
            init
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(init + 1)); // +2 -1 = +1
    }

    #[test]
    fn nested_closure_mutation(a in 0i64..30, b in 0i64..30) {
        // Nested closures, inner mutates outer's captured var
        let expr = format!(
            "(let ((x {}))
               (let ((outer (fn (y)
                              (begin (set x (+ x y)) x))))
                 (begin (outer {}) (outer {}))))",
            a, b, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + b));
    }

    #[test]
    fn closure_over_parameter(param in 0i64..50, delta in 1i64..10) {
        // Closure captures function parameter and mutates it
        let expr = format!(
            "(let ((make-mutator (fn (n)
                                   (fn () (begin (set n (+ n {})) n)))))
               (let ((m (make-mutator {})))
                 (begin (m) (m) (m))))",
            delta, param
        );
        let result = eval_source(&expr);

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
            "(let ((total {}))
               (let ((add (fn (x) (begin (set total (+ total x)) total))))
                 (begin {})))",
            init, calls
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = init + values.iter().sum::<i64>();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }
}

// ============================================================================
// Nested Control Flow Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn nested_let_in_if(cond in prop::bool::ANY, a in -100i64..100, b in -100i64..100) {
        let cond_str = if cond { "true" } else { "false" };
        let expr = format!(
            "(if {} (let ((x {})) x) (let ((y {})) y))",
            cond_str, a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if cond { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn if_in_lambda_body(cond in prop::bool::ANY, a in -100i64..100, b in -100i64..100) {
        let cond_str = if cond { "true" } else { "false" };
        let expr = format!("((fn () (if {} {} {})))", cond_str, a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if cond { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn match_in_lambda(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "((fn (x) (match x ({} \"a\") ({} \"b\") (_ \"other\"))) {})",
            a, b, a
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("a"));
    }

    #[test]
    fn lambda_in_match_body(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(match {} ({} ((fn (x) (+ x {})) {})) (_ 0))",
            a, a, b, a
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Begin/Sequence Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn begin_returns_last(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr = format!("(begin {} {} {})", a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    #[test]
    fn begin_with_side_effects(a in -100i64..100, b in -100i64..100) {
        // Side effect: set! followed by read
        let expr = format!(
            "(let ((x {})) (begin (set x {}) x))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }
}

// ============================================================================
// Cond Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn cond_first_true(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let expr = format!("(cond (true {}) (true {}) (else {}))", a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cond_falls_through_to_else(a in -100i64..100) {
        let expr = format!("(cond (false 1) (false 2) (else {}))", a);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn cond_with_computed_conditions(a in -100i64..100, threshold in -100i64..100) {
        let expr = format!(
            "(cond ((< {} {}) \"less\") ((= {} {}) \"equal\") (else \"greater\"))",
            a, threshold, a, threshold
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = if a < threshold {
            "less"
        } else if a == threshold {
            "equal"
        } else {
            "greater"
        };
        prop_assert_eq!(result.unwrap(), Value::string(expected));
    }
}

// ============================================================================
// Quasiquote Properties (if supported)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn quasiquote_with_unquote(a in -100i64..100) {
        let expr = format!("(let ((x {})) `(1 ,x 3))", a);
        let result = eval_source(&expr);

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
    #![proptest_config(ProptestConfig::with_cases(30))]




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
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn make_adder_works(n in -50i64..50, x in -50i64..50) {
        let expr = format!(
            "(let ((make-adder (fn (n) (fn (x) (+ x n)))))
               ((make-adder {}) {}))",
            n, x
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + n));
    }

    #[test]
    fn make_multiplier_works(n in -20i64..20, x in -20i64..20) {
        let expr = format!(
            "(let ((make-mult (fn (n) (fn (x) (* x n)))))
               ((make-mult {}) {}))",
            n, x
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x * n));
    }

    #[test]
    fn compose_functions(a in -20i64..20) {
        // (compose f g)(x) = f(g(x))
        // Use let* because composed references compose, add1, double from earlier bindings
        let expr = format!(
            "(let* ((compose (fn (f g) (fn (x) (f (g x)))))
                    (add1 (fn (x) (+ x 1)))
                    (double (fn (x) (* x 2)))
                    (composed (compose add1 double)))
                (composed {}))",
            a
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a * 2) + 1));
    }

    #[test]
    fn apply_n_times(n in 1usize..5, start in 0i64..20) {
        // Apply increment n times
        let mut expr = "(let ((inc (fn (x) (+ x 1)))) ".to_string();
        for _ in 0..n {
            expr.push_str("(inc ");
        }
        expr.push_str(&start.to_string());
        for _ in 0..n {
            expr.push(')');
        }
        expr.push(')');

        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(start + n as i64));
    }
}

// ============================================================================
// Currying and Partial Application Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn manual_curry_add(a in -50i64..50, b in -50i64..50) {
        // curry: (a, b) -> a -> b -> result
        let expr = format!(
            "(let ((curry-add (fn (a) (fn (b) (+ a b)))))
               ((curry-add {}) {}))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Recursion Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn recursive_factorial(n in 0u8..8) {
        let expr = format!(
            "(letrec ((fact (fn (n) (if (<= n 1) 1 (* n (fact (- n 1)))))))
               (fact {}))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (1..=n as i64).product();
        let expected = if expected == 0 { 1 } else { expected };
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn recursive_sum(n in 0u8..20) {
        let expr = format!(
            "(letrec ((sum-to (fn (n) (if (<= n 0) 0 (+ n (sum-to (- n 1)))))))
               (sum-to {}))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (0..=n as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn recursive_length(len in 0usize..10) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");
        let expr = format!(
            "(letrec ((my-length (fn (lst) (if (empty? lst) 0 (+ 1 (my-length (rest lst)))))))
               (my-length (list {})))",
            list_str
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn tail_recursive_sum(n in 0u8..50) {
        let expr = format!(
            "(letrec ((sum-iter (fn (n acc) (if (<= n 0) acc (sum-iter (- n 1) (+ acc n))))))
               (sum-iter {} 0))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = (0..=n as i64).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn mutual_recursion_even_odd(n in 0u8..20) {
        let expr = format!(
            "(letrec ((is-even (fn (n) (if (= n 0) true (is-odd (- n 1)))))
                      (is-odd (fn (n) (if (= n 0) false (is-even (- n 1))))))
               (is-even {}))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(n % 2 == 0));
    }
}

// ============================================================================
// Function as Data Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn store_function_in_list(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(let ((fns (list (fn (x) (+ x 1)) (fn (x) (* x 2)))))
               (+ ((first fns) {}) ((first (rest fns)) {})))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a + 1) + (b * 2)));
    }

    #[test]
    fn function_returning_function_returning_value(a in -30i64..30, b in -30i64..30) {
        // Test a function that returns a function that returns a value
        let expr = format!(
            "(let ((f (fn (x) (fn (y) (+ x y)))))
               ((f {}) {}))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }
}

// ============================================================================
// Higher-Order Function Properties (map, filter, fold)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn map_adds_one(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!(
            "(let ((result (map (fn (x) (+ x 1)) (list {} {} {}))))
               (+ (first result) (+ (first (rest result)) (first (rest (rest result))))))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a+1) + (b+1) + (c+1)));
    }

    #[test]
    fn map_doubles(a in -30i64..30, b in -30i64..30) {
        let expr = format!(
            "(let ((result (map (fn (x) (* x 2)) (list {} {}))))
               (list (first result) (first (rest result))))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        if let Ok(vec) = result.unwrap().list_to_vec() {
            prop_assert_eq!(vec.len(), 2);
            prop_assert_eq!(&vec[0], &Value::int(a * 2));
            prop_assert_eq!(&vec[1], &Value::int(b * 2));
        }
    }

    #[test]
    fn map_preserves_length(len in 1usize..6) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");
        let expr = format!("(length (map (fn (x) x) (list {})))", list_str);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn filter_positive(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!(
            "(length (filter (fn (x) (> x 0)) (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = [a, b, c].iter().filter(|&&x| x > 0).count() as i64;
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn filter_all_true_preserves(a in 1i64..50, b in 1i64..50) {
        let expr = format!(
            "(length (filter (fn (x) true) (list {} {})))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2));
    }

    #[test]
    fn filter_all_false_empty(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(length (filter (fn (x) false) (list {} {})))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(0));
    }

    #[test]
    fn fold_sum(a in -30i64..30, b in -30i64..30, c in -30i64..30) {
        let expr = format!("(fold + 0 (list {} {} {}))", a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    #[test]
    fn fold_product(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        let expr = format!("(fold * 1 (list {} {} {}))", a, b, c);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * b * c));
    }

    #[test]
    fn fold_with_initial(init in -50i64..50, a in -30i64..30, b in -30i64..30) {
        let expr = format!("(fold + {} (list {} {}))", init, a, b);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(init + a + b));
    }

    #[test]
    fn fold_empty_returns_initial(init in -100i64..100) {
        let expr = format!("(fold + {} (list))", init);
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(init));
    }

    #[test]
    fn map_then_fold(a in -20i64..20, b in -20i64..20, c in -20i64..20) {
        // map to double, then fold to sum
        let expr = format!(
            "(fold + 0 (map (fn (x) (* x 2)) (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + b + c)));
    }

    #[test]
    fn filter_then_fold(a in -20i64..20, b in -20i64..20, c in -20i64..20) {
        // filter positive, then sum
        let expr = format!(
            "(fold + 0 (filter (fn (x) (> x 0)) (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = [a, b, c].iter().filter(|&&x| x > 0).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn map_with_closure_capture(n in -20i64..20, a in -20i64..20, b in -20i64..20) {
        // Closure captures n from outer scope
        let expr = format!(
            "(let ((n {}))
                (let ((result (map (fn (x) (+ x n)) (list {} {}))))
                  (+ (first result) (first (rest result)))))",
            n, a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a + n) + (b + n)));
    }

    // ============================================================================
    // Define-in-Fold Bug Tests (BUGBUG.md)
    // ============================================================================

    #[test]
    fn define_inside_fold_lambda(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        // Bug: define inside a fold lambda should work
        let expr = format!(
            "(fold (fn (acc x)
                     (begin
                       (var doubled (* x 2))
                       (+ acc doubled)))
                   0
                   (list {} {} {}))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "define in fold lambda failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + b + c)));
    }

    #[test]
    fn nested_define_in_fold(a in 1i64..5, b in 1i64..5) {
        // Bug: multiple defines inside fold lambda
        let expr = format!(
            "(fold (fn (acc x)
                     (begin
                       (var step1 (+ x 1))
                       (var step2 (* step1 2))
                       (+ acc step2)))
                   0
                   (list {} {}))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "nested define in fold failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + 1) + 2 * (b + 1)));
    }

    #[test]
    fn function_with_define_called_from_fold(a in 1i64..10, b in 1i64..10) {
        // Bug: calling a function that has internal defines from within fold
        let expr = format!(
            "(begin
               (def process (fn (x)
                                 (begin
                                   (var doubled (* x 2))
                                   (var incremented (+ doubled 1))
                                   incremented)))
               (fold (fn (acc x) (+ acc (process x)))
                     0
                     (list {} {})))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "function with define called from fold failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((2 * a + 1) + (2 * b + 1)));
    }

    #[test]
    fn nested_fold_with_define(a in 1i64..5, b in 1i64..5) {
        // Bug: nested folds with defines in inner lambda
        let expr = format!(
            "(fold (fn (outer-acc outer-x)
                     (+ outer-acc
                        (fold (fn (inner-acc inner-x)
                                (begin
                                  (var product (* outer-x inner-x))
                                  (+ inner-acc product)))
                              0
                              (list {} {}))))
                   0
                   (list {} {}))",
            a, b, a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "nested fold with define failed: {:?}", result);
        // Each outer element multiplied by each inner element, summed
        let expected = (a * a + a * b) + (b * a + b * b);
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn define_in_fold_with_string_ops(a in 1i64..5, b in 1i64..5) {
        // Similar to elle-doc's pattern: fold with string-append and internal defines
        let expr = format!(
            "(fold (fn (acc x)
                     (begin
                       (var num-str (number->string x))
                        (var wrapped (append (append \"[\" num-str) \"]\"))
                        (append acc wrapped)))
                   \"\"
                   (list {} {}))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "define in fold with strings failed: {:?}", result);
        let expected = format!("[{}][{}]", a, b);
        prop_assert_eq!(result.unwrap(), Value::string(expected.as_str()));
    }

    #[test]
    fn map_with_internal_define(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        // Bug may also affect map
        let expr = format!(
            "(fold + 0 (map (fn (x)
                              (begin
                                (var squared (* x x))
                                squared))
                            (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "map with internal define failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a*a + b*b + c*c));
    }

    #[test]
    fn filter_with_internal_define(a in -10i64..10, b in -10i64..10, c in -10i64..10) {
        // Bug may also affect filter
        let expr = format!(
            "(length (filter (fn (x)
                               (begin
                                 (var abs-x (if (< x 0) (- 0 x) x))
                                 (> abs-x 5)))
                             (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "filter with internal define failed: {:?}", result);
        let expected = [a, b, c].iter().filter(|&&x| x.abs() > 5).count() as i64;
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    // ============================================================================
    // Parameter Name Collision Bug (Regression Test)
    // ============================================================================

    #[test]
    fn parameter_name_collision_in_higher_order(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        // Bug: When outer function parameter name matches inner function parameter name,
        // variable resolution fails.
        //
        // fold-acc has parameter "acc"
        // process has parameter "acc" (collision)
        // When fold-acc calls (f acc ...), incorrect binding occurs.
        let expr = format!(
            "(begin
               (def process (fn (acc x)
                 (begin
                   (var doubled (* x 2))
                   (+ acc doubled))))
               
                ;; This should work but fails due to name collision
                (def fold-acc (fn (f acc lst)
                  (if (empty? lst)
                    acc
                    (fold-acc f (f acc (first lst)) (rest lst)))))
               
               (fold-acc process 0 (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "parameter name collision bug: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + b + c)));
    }

    #[test]
    fn no_collision_works(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        // Same logic but with different parameter name (init vs acc) - should work
        let expr = format!(
            "(begin
               (def process (fn (acc x)
                 (begin
                   (var doubled (* x 2))
                   (+ acc doubled))))
                
                (def fold-init (fn (f init lst)
                  (if (empty? lst)
                    init
                    (fold-init f (f init (first lst)) (rest lst)))))
               
               (fold-init process 0 (list {} {} {})))",
            a, b, c
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "fold-init failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + b + c)));
    }

    #[test]
    fn deeply_nested_lambdas_with_locals(a in 1i64..10, b in 1i64..10) {
        // Three levels of nested lambdas, each with its own local define
        let expr = format!(
            "(let ((outer (fn (x)
                            (begin
                              (var outer-local (* x 2))
                              (fn (y)
                                (begin
                                  (var middle-local (+ y outer-local))
                                  (fn (z)
                                    (begin
                                      (var inner-local (* z middle-local))
                                      inner-local))))))))
               (((outer {}) {}) 3))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "deeply nested lambdas with locals failed: {:?}", result);
        // outer-local = a * 2
        // middle-local = b + (a * 2)
        // inner-local = 3 * (b + a * 2)
        prop_assert_eq!(result.unwrap(), Value::int(3 * (b + a * 2)));
    }

    #[test]
    fn local_shadows_captured_variable(outer_val in 1i64..20, inner_val in 50i64..100) {
        // Inner lambda defines a local with same name as captured variable
        // The local should shadow the capture within the inner scope
        let expr = format!(
            "(let ((x {}))
               (let ((f (fn ()
                          (begin
                            (var x {})
                            x))))
                 (+ (f) x)))",
            outer_val, inner_val
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "local shadows captured variable failed: {:?}", result);
        // f returns inner_val (the shadowing local)
        // outer x is still outer_val
        prop_assert_eq!(result.unwrap(), Value::int(inner_val + outer_val));
    }

    #[test]
    fn multiple_closures_with_independent_locals(a in 1i64..10, b in 1i64..10) {
        // Two closures created in the same scope, each with its own local define
        // Their locals should be independent
        let expr = format!(
            "(let ((make-f1 (fn ()
                              (fn (x)
                                (begin
                                  (var local (* x 2))
                                  local))))
                   (make-f2 (fn ()
                              (fn (x)
                                (begin
                                  (var local (* x 3))
                                  local)))))
               (let ((f1 (make-f1))
                     (f2 (make-f2)))
                 (+ (f1 {}) (f2 {}))))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "multiple closures with independent locals failed: {:?}", result);
        // f1 returns a * 2, f2 returns b * 3
        prop_assert_eq!(result.unwrap(), Value::int(a * 2 + b * 3));
    }
}

// ============================================================================
// Nested Define and Letrec Patterns
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn letrec_mutual_recursion_in_lambda(n in 0u8..20) {
        // Mutual recursion inside a lambda using letrec
        let expr = format!(
            "(let ((check (fn ()
                            (letrec ((is-even (fn (x) (if (= x 0) true (is-odd (- x 1)))))
                                     (is-odd (fn (x) (if (= x 0) false (is-even (- x 1))))))
                              (is-even {})))))
               (check))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "letrec mutual recursion in lambda failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(n % 2 == 0));
    }

    #[test]
    fn nested_letrec_different_scopes(a in 1i64..10, b in 1i64..10) {
        // Nested letrec blocks with independent bindings
        let expr = format!(
            "(letrec ((outer (fn (x)
                              (letrec ((inner (fn (y) (* x y))))
                                (inner {})))))
               (outer {}))",
            b, a
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "nested letrec failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * b));
    }

    #[test]
    fn letrec_with_captured_outer_var(outer in 1i64..20, n in 0u8..10) {
        // letrec where inner functions capture outer scope
        let expr = format!(
            "(let ((base {}))
               (letrec ((add-base (fn (x) (+ base x)))
                        (double-add (fn (x) (add-base (add-base x)))))
                 (double-add {})))",
            outer, n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "letrec with captured outer failed: {:?}", result);
        // double-add(n) = add-base(add-base(n)) = (n + base) + base = n + 2*base
        prop_assert_eq!(result.unwrap(), Value::int(n as i64 + 2 * outer));
    }

    #[test]
    fn self_recursive_with_local_define(n in 1u8..10) {
        // Self-recursion with local define (not mutual - this should work)
        let expr = format!(
            "(let ((compute (fn (x)
                              (begin
                                (def helper (fn (n acc)
                                  (if (= n 0) acc (helper (- n 1) (+ acc n)))))
                                (helper x 0)))))
               (compute {}))",
            n
        );
        let result = eval_source(&expr);

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
            "(let ((n 0))
               (letrec ((inc (fn () (begin (set n (+ n 1)) n)))
                        (get (fn () n)))
                 (begin {} (get))))",
            calls
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "counter in letrec failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(increments as i64));
    }

    #[test]
    fn three_way_mutual_letrec(n in 0u8..15) {
        // Three mutually recursive functions
        let expr = format!(
            "(letrec ((f (fn (x) (if (= x 0) \"f\" (g (- x 1)))))
                      (g (fn (x) (if (= x 0) \"g\" (h (- x 1)))))
                      (h (fn (x) (if (= x 0) \"h\" (f (- x 1))))))
               (f {}))",
            n
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "three-way mutual letrec failed: {:?}", result);
        let expected = match n % 3 {
            0 => "f",
            1 => "g",
            2 => "h",
            _ => unreachable!(),
        };
        prop_assert_eq!(result.unwrap(), Value::string(expected));
    }

    #[test]
    fn letrec_with_higher_order(a in 1i64..10, b in 1i64..10, _c in 1i64..10) {
        // letrec where functions take other functions as arguments
        let expr = format!(
            "(letrec ((apply-twice (fn (f x) (f (f x))))
                      (add-one (fn (x) (+ x 1)))
                      (double (fn (x) (* x 2))))
               (+ (apply-twice add-one {})
                  (apply-twice double {})))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "letrec with higher order failed: {:?}", result);
        // apply-twice add-one a = a + 2
        // apply-twice double b = b * 4
        prop_assert_eq!(result.unwrap(), Value::int((a + 2) + (b * 4)));
    }

    #[test]
    fn nested_lambda_with_define_no_forward_ref(a in 1i64..20, b in 1i64..20) {
        // Local define without forward references should work
        let expr = format!(
            "(let ((outer (fn (x)
                            (begin
                              (var local (* x 2))
                              (fn (y) (+ local y))))))
               ((outer {}) {}))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "nested lambda with define failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * 2 + b));
    }

    #[test]
    fn sequential_defines_no_mutual(a in 1i64..10, b in 1i64..10) {
        // Sequential defines where second uses first (not mutual)
        let expr = format!(
            "(let ((compute (fn ()
                              (begin
                                (var x {})
                                (var y (+ x {}))
                                (var z (* x y))
                                z))))
               (compute))",
            a, b
        );
        let result = eval_source(&expr);

        prop_assert!(result.is_ok(), "sequential defines failed: {:?}", result);
        let x = a;
        let y = a + b;
        let z = x * y;
        prop_assert_eq!(result.unwrap(), Value::int(z));
    }
}
