// Property-based tests for bug fixes
//
// These tests verify that three bug fixes remain correct across a wide range
// of inputs using property-based testing:
// 1. StoreCapture stack mismatch (let bindings inside lambdas)
// 2. defn function definition syntax
// 3. List display (no `. ()` in proper lists)

use crate::common::eval_source;
use elle::Value;
use proptest::prelude::*;

// ============================================================================
// Bug 1: StoreCapture stack mismatch (let bindings inside lambdas)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: let binding inside lambda preserves the bound value
    #[test]
    fn let_in_lambda_preserves_value(x in -1000i64..1000) {
        let code = format!(
            "(def f (fn (x) (let ((y x)) y))) (f {})", x
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }

    /// Property: let binding inside lambda doesn't corrupt subsequent operations
    #[test]
    fn let_in_lambda_with_arithmetic(a in -100i64..100, b in -100i64..100) {
        let code = format!(
            "(def f (fn (a b) (let ((x a) (y b)) (+ x y)))) (f {} {})", a, b
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: recursive function with let inside lambda produces correct list
    #[test]
    fn recursive_let_in_lambda_list_length(n in 0usize..20) {
        let code = format!(
            "(def f (fn (x) (if (= x 0) (list) (let ((y x)) (cons y (f (- x 1))))))) (length (f {}))", n
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n as i64));
    }

    /// Property: append inside let inside lambda works for arbitrary list sizes
    #[test]
    fn append_in_let_in_lambda(n in 0usize..15) {
        let code = format!(
            "(def f (fn (x) (if (= x 0) (list) (let ((y x)) (append (list y) (f (- x 1))))))) (length (f {}))", n
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n as i64));
    }

    /// Property: multiple let bindings don't corrupt stack
    #[test]
    fn multiple_let_bindings_in_lambda(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let code = format!(
            "(def f (fn (a b c) (let ((x a) (y b) (z c)) (+ x (+ y z))))) (f {} {} {})", a, b, c
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    /// Property: nested let bindings inside lambda work correctly
    #[test]
    fn nested_let_in_lambda(x in -100i64..100, y in -100i64..100) {
        let code = format!(
            "(def f (fn (a b) (let ((x a)) (let ((y b)) (+ x y))))) (f {} {})", x, y
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + y));
    }

    /// Property: let binding with computation inside lambda
    #[test]
    fn let_with_computation_in_lambda(x in -50i64..50) {
        let code = format!(
            "(def f (fn (x) (let ((y (* x 2)) (z (+ x 1))) (+ y z)))) (f {})", x
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        // y = x * 2, z = x + 1, result = y + z = 3x + 1
        prop_assert_eq!(result.unwrap(), Value::int(3 * x + 1));
    }
}

// ============================================================================
// Bug 2: defn
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: (defn f (x) body) is equivalent to (def f (fn (x) body))
    #[test]
    fn define_shorthand_equivalent(x in -1000i64..1000) {
        let shorthand = format!("(defn f (x) (+ x 1)) (f {})", x);
        let longhand = format!("(def f (fn (x) (+ x 1))) (f {})", x);
        let r1 = eval_source(&shorthand);
        let r2 = eval_source(&longhand);
        prop_assert!(r1.is_ok(), "Shorthand failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Longhand failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    /// Property: shorthand with multiple params matches longhand
    #[test]
    fn define_shorthand_multi_param(a in -100i64..100, b in -100i64..100) {
        let shorthand = format!("(defn add (a b) (+ a b)) (add {} {})", a, b);
        let longhand = format!("(def add (fn (a b) (+ a b))) (add {} {})", a, b);
        let r1 = eval_source(&shorthand);
        let r2 = eval_source(&longhand);
        prop_assert!(r1.is_ok(), "Shorthand failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Longhand failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    /// Property: shorthand recursive functions work (factorial)
    #[test]
    fn define_shorthand_recursive(n in 0u64..12) {
        let expected: u64 = (1..=n).product();
        let expected = if n == 0 { 1 } else { expected };
        let code = format!(
            "(defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact {})", n
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(expected as i64));
    }

    /// Property: shorthand with three params matches longhand
    #[test]
    fn define_shorthand_three_params(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let shorthand = format!("(defn sum3 (a b c) (+ a (+ b c))) (sum3 {} {} {})", a, b, c);
        let longhand = format!("(def sum3 (fn (a b c) (+ a (+ b c)))) (sum3 {} {} {})", a, b, c);
        let r1 = eval_source(&shorthand);
        let r2 = eval_source(&longhand);
        prop_assert!(r1.is_ok(), "Shorthand failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Longhand failed: {:?}", r2);
        prop_assert_eq!(r1.unwrap(), r2.unwrap());
    }

    /// Property: shorthand with conditional body
    #[test]
    fn define_shorthand_conditional(x in -100i64..100) {
        let shorthand = format!("(defn abs (x) (if (< x 0) (- 0 x) x)) (abs {})", x);
        let longhand = format!("(def abs (fn (x) (if (< x 0) (- 0 x) x))) (abs {})", x);
        let r1 = eval_source(&shorthand);
        let r2 = eval_source(&longhand);
        prop_assert!(r1.is_ok(), "Shorthand failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Longhand failed: {:?}", r2);
        let v1 = r1.unwrap();
        let v2 = r2.unwrap();
        prop_assert_eq!(v1, v2);
        prop_assert_eq!(v1, Value::int(x.abs()));
    }

    /// Property: shorthand with let body
    #[test]
    fn define_shorthand_with_let(x in -100i64..100) {
        let shorthand = format!("(defn double (x) (let ((y x)) (+ y y))) (double {})", x);
        let longhand = format!("(def double (fn (x) (let ((y x)) (+ y y)))) (double {})", x);
        let r1 = eval_source(&shorthand);
        let r2 = eval_source(&longhand);
        prop_assert!(r1.is_ok(), "Shorthand failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Longhand failed: {:?}", r2);
        let v1 = r1.unwrap();
        let v2 = r2.unwrap();
        prop_assert_eq!(v1, v2);
        prop_assert_eq!(v1, Value::int(x * 2));
    }
}

// ============================================================================
// Bug 3: List display (no `. ()`)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: list of integers displays without ". ()"
    #[test]
    fn list_display_no_dot_terminator(xs in prop::collection::vec(-100i64..100, 0..10)) {
        let elements = xs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
        let code = if xs.is_empty() {
            "(list)".to_string()
        } else {
            format!("(list {})", elements)
        };
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "List display contained '. ()': {}", display);
        // Also verify it starts with ( and ends with )
        prop_assert!(display.starts_with('('), "Display should start with '(': {}", display);
        prop_assert!(display.ends_with(')'), "Display should end with ')': {}", display);
    }

    /// Property: cons chain terminated by (list) displays as proper list
    #[test]
    fn cons_chain_display_proper(n in 1usize..10) {
        let mut code = "(list)".to_string();
        for i in (1..=n).rev() {
            code = format!("(cons {} {})", i, code);
        }
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "Cons chain display contained '. ()': {}", display);
    }

    /// Property: length of list matches input count
    #[test]
    fn list_length_matches_input(n in 0usize..10) {
        let elements = (1..=n).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        let code = if n == 0 {
            "(length (list))".to_string()
        } else {
            format!("(length (list {}))", elements)
        };
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n as i64));
    }

    /// Property: nested list display has no ". ()"
    #[test]
    fn nested_list_display_no_dot(depth in 1usize..5) {
        // Build nested lists: (list (list (list ...)))
        let mut code = "(list)".to_string();
        for _ in 0..depth {
            code = format!("(list {})", code);
        }
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "Nested list display contained '. ()': {}", display);
    }

    /// Property: list with mixed positive and negative integers displays correctly
    #[test]
    fn list_mixed_integers_display(
        pos in prop::collection::vec(1i64..100, 1..5),
        neg in prop::collection::vec(-100i64..-1, 1..5)
    ) {
        let mut all: Vec<i64> = pos.into_iter().chain(neg.into_iter()).collect();
        all.sort(); // Deterministic order
        let elements = all.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
        let code = format!("(list {})", elements);
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "Mixed list display contained '. ()': {}", display);
    }

    /// Property: append result displays without ". ()"
    #[test]
    fn append_result_display_no_dot(
        xs in prop::collection::vec(-50i64..50, 0..5),
        ys in prop::collection::vec(-50i64..50, 0..5)
    ) {
        let xs_str = xs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
        let ys_str = ys.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
        let code = format!(
            "(append (list {}) (list {}))",
            xs_str, ys_str
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "Append result display contained '. ()': {}", display);
    }
}

// ============================================================================
// Bug 4: or expression corrupts return value in recursive calls
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: or in recursive check function doesn't corrupt append arguments
    #[test]
    fn or_in_recursive_check_with_append(n in 3usize..8) {
        let code = format!(r#"
            (var check
              (fn (x remaining)
                (if (empty? remaining)
                  #t
                  (if (or (= x 1) (= x 2))
                    #f
                    (check x (rest remaining))))))

            (var foo
              (fn (n seen)
                (if (= n 0)
                  (list)
                  (if (check n seen)
                    (append (list n) (foo (- n 1) (cons n seen)))
                    (foo (- n 1) seen)))))

            (length (foo {} (list 0)))
        "#, n);
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        // n=3: 3 is safe, 2 is not, 1 is not, 0 is base -> (3) -> length 1
        // n=4: 4,3 are safe, 2,1 are not -> (4 3) -> length 2
        // n=5: 5,4,3 are safe -> (5 4 3) -> length 3
        // Pattern: n - 2 elements (since 1 and 2 are filtered out)
        let expected = if n >= 3 { n - 2 } else { 0 };
        prop_assert_eq!(result.unwrap(), Value::int(expected as i64));
    }
}

// ============================================================================
// Combined property tests: interactions between fixes
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: define shorthand with let inside produces correct list display
    #[test]
    fn shorthand_with_let_list_display(n in 1usize..10) {
        let code = format!(
            "(defn make-list (x) (if (= x 0) (list) (let ((y x)) (cons y (make-list (- x 1)))))) (make-list {})", n
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        let display = format!("{}", result.unwrap());
        prop_assert!(!display.contains(". ()"),
            "List from shorthand+let contained '. ()': {}", display);
    }

    /// Property: recursive shorthand with let produces correct values
    #[test]
    fn shorthand_recursive_with_let(n in 0usize..15) {
        // Build list using shorthand define with let inside
        let code = format!(
            "(defn build (n) (if (= n 0) (list) (let ((rest (build (- n 1)))) (cons n rest)))) (length (build {}))", n
        );
        let result = eval_source(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n as i64));
    }
}
