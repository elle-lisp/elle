use super::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(30))]

    #[test]
    fn map_adds_one(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!(
            "(let [result (map (fn (x) (+ x 1)) (list {} {} {}))]
               (+ (first result) (+ (first (rest result)) (first (rest (rest result))))))",
            a, b, c
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int((a+1) + (b+1) + (c+1)));
    }

    #[test]
    fn map_doubles(a in -30i64..30, b in -30i64..30) {
        let expr = format!(
            "(let [result (map (fn (x) (* x 2)) (list {} {}))]
               (list (first result) (first (rest result))))",
            a, b
        );
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn filter_positive(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let expr = format!(
            "(length (filter (fn (x) (> x 0)) (list {} {} {})))",
            a, b, c
        );
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2));
    }

    #[test]
    fn filter_all_false_empty(a in -50i64..50, b in -50i64..50) {
        let expr = format!(
            "(length (filter (fn (x) false) (list {} {})))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(0));
    }

    #[test]
    fn fold_sum(a in -30i64..30, b in -30i64..30, c in -30i64..30) {
        let expr = format!("(fold + 0 (list {} {} {}))", a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    #[test]
    fn fold_product(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        let expr = format!("(fold * 1 (list {} {} {}))", a, b, c);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a * b * c));
    }

    #[test]
    fn fold_with_initial(init in -50i64..50, a in -30i64..30, b in -30i64..30) {
        let expr = format!("(fold + {} (list {} {}))", init, a, b);
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(init + a + b));
    }

    #[test]
    fn fold_empty_returns_initial(init in -100i64..100) {
        let expr = format!("(fold + {} (list))", init);
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected: i64 = [a, b, c].iter().filter(|&&x| x > 0).sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn map_with_closure_capture(n in -20i64..20, a in -20i64..20, b in -20i64..20) {
        // Closure captures n from outer scope
        let expr = format!(
            "(let [n {}]
                (let [result (map (fn (x) (+ x n)) (list {} {}))]
                  (+ (first result) (first (rest result)))))",
            n, a, b
        );
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "nested fold with define failed: {:?}", result);
        // Each outer element multiplied by each inner element, summed
        let expected = (a * a + a * b) + (b * a + b * b);
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn define_in_fold_with_string_ops(a in 1i64..5, b in 1i64..5) {
        // Similar to elle-doc's pattern: fold with string-append and internal defines
        let h = elle::primitives::ctx::TestHeap::new();
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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "define in fold with strings failed: {:?}", result);
        let expected = format!("[{}][{}]", a, b);
        prop_assert_eq!(result.unwrap(), h.ctx().string(expected.as_str()));
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
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

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
               
                ## This should work but fails due to name collision
                (def fold-acc (fn (f acc lst)
                  (if (empty? lst)
                    acc
                    (fold-acc f (f acc (first lst)) (rest lst)))))
               
               (fold-acc process 0 (list {} {} {})))",
            a, b, c
        );
        let result = eval_reuse(&expr);

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
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "fold-init failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2 * (a + b + c)));
    }

    #[test]
    fn deeply_nested_lambdas_with_locals(a in 1i64..10, b in 1i64..10) {
        // Three levels of nested lambdas, each with its own local define
        let expr = format!(
            "(let [outer (fn (x)
                            (begin
                              (var outer-local (* x 2))
                              (fn (y)
                                (begin
                                  (var middle-local (+ y outer-local))
                                  (fn (z)
                                    (begin
                                      (var inner-local (* z middle-local))
                                      inner-local))))))]
               (((outer {}) {}) 3))",
            a, b
        );
        let result = eval_reuse(&expr);

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
            "(let [x {}]
               (let [f (fn ()
                          (begin
                            (var x {})
                            x))]
                 (+ (f) x)))",
            outer_val, inner_val
        );
        let result = eval_reuse(&expr);

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
            "(let [make-f1 (fn ()
                              (fn (x)
                                (begin
                                  (var local (* x 2))
                                  local))) make-f2 (fn ()
                              (fn (x)
                                (begin
                                  (var local (* x 3))
                                  local)))]
               (let [f1 (make-f1) f2 (make-f2)]
                 (+ (f1 {}) (f2 {}))))",
            a, b
        );
        let result = eval_reuse(&expr);

        prop_assert!(result.is_ok(), "multiple closures with independent locals failed: {:?}", result);
        // f1 returns a * 2, f2 returns b * 3
        prop_assert_eq!(result.unwrap(), Value::int(a * 2 + b * 3));
    }
}
