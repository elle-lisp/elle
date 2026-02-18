// Property-based tests for the new compilation pipeline
//
// These tests verify semantic correctness by checking mathematical and logical
// properties hold when code is compiled and executed through the new pipeline.
// This file replaces coverage currently only in old-pipeline tests.

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::eval_new;
use elle::primitives::{init_stdlib, register_primitives};
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);

    // Set the symbol table in thread-local context for primitives that need it
    // (e.g., type-of needs to intern type names as keywords)
    set_symbol_table(&mut symbols as *mut SymbolTable);

    eval_new(input, &mut symbols, &mut vm)
}

// ============================================================================
// 1. String Operations (50 cases)
// ============================================================================
// Note: The new pipeline uses polymorphic `length` instead of `string-length`

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn string_length_of_append_equals_sum(
        a in "[a-z]{1,10}",
        b in "[a-z]{1,10}"
    ) {
        // Use polymorphic `length` instead of `string-length`
        let expr = format!(
            "(= (length (string-append \"{}\" \"{}\")) (+ (length \"{}\") (length \"{}\")))",
            a, b, a, b
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn substring_roundtrip(s in "[a-z]{1,10}") {
        let expr = format!(
            "(let ((s \"{}\")) (= (substring s 0 (length s)) s))",
            s
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn string_upcase_downcase_identity(s in "[a-z]{1,10}") {
        // string-upcase then string-downcase of lowercase string is identity
        let expr = format!(
            "(= (string-downcase (string-upcase \"{}\")) \"{}\")",
            s, s
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn string_length_correct(s in "[a-z]{1,10}") {
        let expected_len = s.len() as i64;
        let expr = format!("(length \"{}\")", s);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(expected_len));
    }
}

// ============================================================================
// 2. Type System (100 cases)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn type_of_integer_returns_keyword(n in -1000i64..1000) {
        // type-of returns a keyword for integers
        let expr = format!("(type-of {})", n);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_keyword(), "expected keyword for type-of integer");
    }

    #[test]
    fn type_of_float_returns_keyword(n in -1000.0f64..1000.0) {
        // Ensure we have a decimal point for float literal
        let float_str = if n.fract() == 0.0 {
            format!("{}.0", n as i64)
        } else {
            format!("{}", n)
        };
        let expr = format!("(type-of {})", float_str);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_keyword(), "expected keyword for type-of float");
    }

    #[test]
    fn type_of_string_returns_keyword(s in "[a-z]{1,10}") {
        let expr = format!("(type-of \"{}\")", s);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_keyword(), "expected keyword for type-of string");
    }

    #[test]
    fn type_of_bool_returns_keyword(b in prop::bool::ANY) {
        let bool_str = if b { "#t" } else { "#f" };
        let expr = format!("(type-of {})", bool_str);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_keyword(), "expected keyword for type-of bool");
    }

    #[test]
    fn int_conversion_truncates(n in -100.0f64..100.0) {
        let expected = n.trunc() as i64;
        // Ensure we have a decimal point for float literal
        let float_str = if n.fract() == 0.0 {
            format!("{}.0", n as i64)
        } else {
            format!("{}", n)
        };
        let expr = format!("(int {})", float_str);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn float_conversion_preserves_int(n in -1000i64..1000) {
        // (float n) should equal n when compared as float
        let expr = format!("(= (float {}) {}.0)", n, n);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn int_plus_float_produces_float(i in -100i64..100, f in 0.1f64..10.0) {
        // Check that the result is a float by verifying it's not equal to its truncated value
        // (since int + non-integer float should produce a non-integer float)
        let expr = format!("(let ((r (+ {} {}))) (not (= r (int r))))", i, f);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn type_error_on_add_nil(n in -100i64..100) {
        let expr = format!("(+ {} nil)", n);
        let result = eval(&expr);

        prop_assert!(result.is_err(), "expected error for (+ {} nil)", n);
    }
}

// ============================================================================
// 3. Tables and Structs (30 cases)
// ============================================================================
// Note: The new pipeline uses polymorphic `get`/`put` instead of `table-get`/`table-set!`
// and `table?`/`struct?` predicates are not yet registered.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn table_get_after_put_returns_value(key in 1i64..100, value in -1000i64..1000) {
        // Use polymorphic `put` and `get` instead of `table-set!` and `table-get`
        let expr = format!(
            "(let ((t (table))) (begin (put t {} {}) (get t {})))",
            key, value, key
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(value));
    }

    #[test]
    fn table_creation_returns_table(_dummy in 0i64..1) {
        // Just verify table creation doesn't error
        let expr = "(table)";
        let result = eval(expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_table(), "expected table");
    }

    #[test]
    fn struct_creation_returns_struct(_dummy in 0i64..1) {
        // Just verify struct creation doesn't error
        let expr = "(struct)";
        let result = eval(expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert!(result.unwrap().is_struct(), "expected struct");
    }

    #[test]
    fn table_multiple_puts(k1 in 1i64..50, v1 in -100i64..100, k2 in 51i64..100, v2 in -100i64..100) {
        let expr = format!(
            "(let ((t (table)))
               (begin
                 (put t {} {})
                 (put t {} {})
                 (+ (get t {}) (get t {}))))",
            k1, v1, k2, v2, k1, k2
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v1 + v2));
    }
}

// ============================================================================
// 4. Pattern Matching — Extended (50 cases)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn match_variable_binding_int(n in -100i64..100) {
        let expr = format!("(match {} (x (+ x 1)))", n);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n + 1));
    }

    #[test]
    fn match_variable_binding_string(s in "[a-z]{1,10}") {
        let expr = format!("(match \"{}\" (x (string-append x \"!\")))", s);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        let expected = format!("{}!", s);
        prop_assert_eq!(result.unwrap(), Value::string(expected));
    }

    #[test]
    fn match_nested(a in -50i64..50, b in -50i64..50) {
        let expr = format!("(match {} (x (match {} (y (+ x y)))))", a, b);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    #[test]
    fn match_variable_shadowing(a in -50i64..50, b in -50i64..50) {
        // Inner x shadows outer x
        let expr = format!("(match {} (x (match {} (x x))))", a, b);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn match_float_literal_exact(f in 1.0f64..10.0) {
        // Only exact match hits
        let expr = format!("(match {} ({} \"hit\") (_ \"miss\"))", f, f);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("hit"));
    }

    #[test]
    fn match_with_computed_scrutinee(a in -50i64..50, b in -50i64..50) {
        let sum = a + b;
        let expr = format!("(match (+ {} {}) ({} \"hit\") (_ \"miss\"))", a, b, sum);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string("hit"));
    }
}

// ============================================================================
// 5. Exception/Condition Handling (30 cases)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn handler_case_no_error_returns_body(n in -100i64..100) {
        let expr = format!("(handler-case {} (error e -1))", n);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(n));
    }

    #[test]
    fn handler_case_catches_division_by_zero(a in 1i64..100, sentinel in -1000i64..-1) {
        let expr = format!("(handler-case (/ {} 0) (error e {}))", a, sentinel);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(sentinel));
    }

    #[test]
    fn nested_handler_case_inner_catches(a in 1i64..100) {
        // Inner handler catches first
        let expr = format!(
            "(handler-case (handler-case (/ {} 0) (error e 50)) (error e 100))",
            a
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(50));
    }

    #[test]
    fn exception_message_roundtrip(s in "[a-z]{1,10}") {
        let expr = format!("(exception-message (exception \"{}\" nil))", s);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string(s.as_str()));
    }

    #[test]
    fn exception_data_roundtrip(data in -100i64..100) {
        let expr = format!("(exception-data (exception \"test\" {}))", data);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(data));
    }
}

// ============================================================================
// 6. Deep Tail Recursion (10 cases — expensive)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    #[test]
    fn countdown_from_n(n in 1000u32..50000) {
        let expr = format!(
            "(begin (define f (fn (n) (if (<= n 0) 0 (f (- n 1))))) (f {}))",
            n
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "countdown failed for n={}: {:?}", n, result);
        prop_assert_eq!(result.unwrap(), Value::int(0));
    }

    #[test]
    fn accumulator_sum(n in 1000u32..10000) {
        // Sum from 1 to n = n*(n+1)/2
        let expected = (n as i64) * ((n as i64) + 1) / 2;
        let expr = format!(
            "(begin
               (define sum-iter (fn (n acc)
                 (if (<= n 0) acc (sum-iter (- n 1) (+ acc n)))))
               (sum-iter {} 0))",
            n
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "accumulator sum failed for n={}: {:?}", n, result);
        prop_assert_eq!(result.unwrap(), Value::int(expected));
    }

    #[test]
    fn mutual_recursion_even_odd(n in 0u32..10000) {
        let expected = n % 2 == 0;
        let expr = format!(
            "(begin
               (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
               (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
               (= (is-even {}) {}))",
            n, if expected { "#t" } else { "#f" }
        );
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "mutual recursion failed for n={}: {:?}", n, result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }
}

// ============================================================================
// 7. Stdlib Integration (30 cases)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn reverse_list_first_element(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        // (first (reverse (list a b c))) = c
        let expr = format!("(first (reverse (list {} {} {})))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    #[test]
    fn append_lists_length(a in -100i64..100, b in -100i64..100) {
        // (length (append (list a) (list b))) = 2
        let expr = format!("(length (append (list {}) (list {})))", a, b);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(2));
    }

    #[test]
    fn nth_returns_correct_element(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        // (nth 0 (list a b c)) = a
        let expr = format!("(nth 0 (list {} {} {}))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn last_returns_last_element(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        // (last (list a b c)) = c
        let expr = format!("(last (list {} {} {}))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    #[test]
    fn map_identity_preserves_length(len in 1usize..6) {
        let elements: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        let list_str = elements.join(" ");
        let expr = format!("(length (map (fn (x) x) (list {})))", list_str);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(len as i64));
    }

    #[test]
    fn filter_all_true_preserves_length(a in 1i64..50, b in 1i64..50, c in 1i64..50) {
        let expr = format!("(length (filter (fn (x) #t) (list {} {} {})))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(3));
    }

    #[test]
    fn fold_sum(a in -30i64..30, b in -30i64..30, c in -30i64..30) {
        let expr = format!("(fold + 0 (list {} {} {}))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    #[test]
    fn take_returns_prefix(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        // (first (take 2 (list a b c))) = a
        let expr = format!("(first (take 2 (list {} {} {})))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a));
    }

    #[test]
    fn drop_removes_prefix(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        // (first (drop 1 (list a b c))) = b
        let expr = format!("(first (drop 1 (list {} {} {})))", a, b, c);
        let result = eval(&expr);

        prop_assert!(result.is_ok(), "failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }
}

// ============================================================================
// 8. Box/Cell Operations (30 cases)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn box_unbox_roundtrip_int(n in -1000i64..1000) {
        // (unbox (box n)) = n
        let expr = format!("(unbox (box {}))", n);
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "box/unbox roundtrip failed for {}: {:?}", n, result);
        prop_assert_eq!(result.unwrap(), Value::int(n));
    }

    #[test]
    fn box_set_then_unbox(a in -100i64..100, b in -100i64..100) {
        // Create box with a, set to b, unbox should give b
        let expr = format!(
            "(let ((b (box {}))) (begin (box-set! b {}) (unbox b)))",
            a, b
        );
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "box-set! failed for a={}, b={}: {:?}", a, b, result);
        prop_assert_eq!(result.unwrap(), Value::int(b));
    }

    #[test]
    fn box_predicate_on_box(n in -100i64..100) {
        // (box? (box n)) = #t
        let expr = format!("(box? (box {}))", n);
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "box? failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(true));
    }

    #[test]
    fn box_predicate_on_non_box(n in -100i64..100) {
        // (box? n) = #f
        let expr = format!("(box? {})", n);
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "box? on non-box failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::bool(false));
    }

    #[test]
    fn box_shared_mutation_via_closure(n in 1i64..50) {
        // Two closures sharing a box see each other's mutations
        let expr = format!(
            "(begin
               (define make-pair
                 (fn ()
                   (let ((b (box 0)))
                     (list (fn () (begin (box-set! b (+ (unbox b) 1)) (unbox b)))
                           (fn () (unbox b))))))
               (define p (make-pair))
               (define inc (first p))
               (define get (first (rest p)))
               (let ((i 0) (result 0))
                 (begin {} (get))))",
            (1..=n).map(|_| "(inc)".to_string()).collect::<Vec<_>>().join(" ")
        );
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "shared box mutation failed for n={}: {:?}", n, result);
        prop_assert_eq!(result.unwrap(), Value::int(n));
    }

    #[test]
    fn box_unbox_roundtrip_string(s in "[a-z]{1,8}") {
        // (unbox (box "s")) = "s"
        let expr = format!("(unbox (box \"{}\"))", s);
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "box/unbox string roundtrip failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::string(&*s));
    }

    #[test]
    fn box_multiple_sets(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        // Multiple box-set! calls, last one wins
        let expr = format!(
            "(let ((b (box 0))) (begin (box-set! b {}) (box-set! b {}) (box-set! b {}) (unbox b)))",
            a, b, c
        );
        let result = eval(&expr);
        prop_assert!(result.is_ok(), "multiple box-set! failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }
}
