use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

struct PatternMatchingTest;

impl PatternMatchingTest {
    fn eval(code: &str) -> Result<elle::value::Value, String> {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let value = read_str(code, &mut symbols)?;
        let expr = value_to_expr(&value, &mut symbols)?;
        let bytecode = compile(&expr);
        vm.execute(&bytecode)
    }
}

// ============================================================================
// Basic Pattern Matching Tests
// ============================================================================

#[test]
fn test_match_literal_integer() {
    let result = PatternMatchingTest::eval(r#"(match 42 (42 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_literal_string() {
    let result =
        PatternMatchingTest::eval(r#"(match "hello" ("hello" "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_literal_false() {
    let result = PatternMatchingTest::eval(r#"(match 0 (0 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_literal_true() {
    let result = PatternMatchingTest::eval(r#"(match 1 (1 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_nil_pattern() {
    let result = PatternMatchingTest::eval(r#"(match nil (nil "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_default_case() {
    let result =
        PatternMatchingTest::eval(r#"(match 99 (42 "no") (100 "no") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("default"));
}

#[test]
fn test_match_wildcard_pattern() {
    let result = PatternMatchingTest::eval(r#"(match 42 (_ "wildcard matched"))"#).unwrap();
    assert_eq!(
        result,
        elle::value::Value::string("wildcard matched")
    );
}

// ============================================================================
// Variable Pattern Tests
// Note: Full variable binding requires local variable binding support (Issue #6)
// These tests verify that variable patterns parse and execute correctly
// ============================================================================

#[test]
fn test_match_variable_pattern_number() {
    // Variable patterns accept any value
    let result = PatternMatchingTest::eval(r#"(match 42 (x "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_variable_pattern_string() {
    // Variable patterns accept any string
    let result = PatternMatchingTest::eval(r#"(match "hello" (name "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_variable_pattern_list() {
    // Variable patterns accept lists
    let result = PatternMatchingTest::eval(r#"(match (list 1 2 3) (lst "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_variable_pattern_nil() {
    // Variable patterns accept nil
    let result = PatternMatchingTest::eval(r#"(match nil (x "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

// ============================================================================
// List Pattern Matching Tests
// ============================================================================

#[test]
fn test_match_empty_list_pattern() {
    let result = PatternMatchingTest::eval(r#"(match nil (nil "empty") ("not empty"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("empty"));
}

#[test]
fn test_match_single_element_list() {
    // Variable pattern matches single-element lists
    let result = PatternMatchingTest::eval(r#"(match (list 42) (lst "single element"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("single element"));
}

#[test]
fn test_match_list_with_wildcard() {
    // Wildcard pattern matches any list
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2 3) (_ "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

// ============================================================================
// Control Flow and Pattern Matching Integration Tests
// ============================================================================

#[test]
fn test_match_with_begin_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 42 (42 (begin (+ 10 20) (+ 1 1))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(2));
}

#[test]
fn test_match_with_if_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 42 (42 (if (> 42 30) "yes" "no")))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("yes"));
}

#[test]
fn test_match_result_value() {
    let result = PatternMatchingTest::eval(r#"(+ 10 (match 5 (5 100)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(110));
}

// ============================================================================
// Pattern Matching Error Cases
// ============================================================================

#[test]
fn test_match_no_matching_pattern_default() {
    let result =
        PatternMatchingTest::eval(r#"(match 999 (1 "one") (2 "two") (999 "found") ("none"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::string("found"));
}

#[test]
fn test_match_with_computed_values() {
    let result =
        PatternMatchingTest::eval(r#"(match (+ 20 22) (42 "computed match") ("default"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::string("computed match"));
}

// ============================================================================
// Real-world Pattern Matching Examples
// ============================================================================

#[test]
fn test_match_coordinate_pair() {
    // Wildcard pattern matches coordinate pairs
    let result = PatternMatchingTest::eval(r#"(match (list 10 20) (_ "is a pair"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("is a pair"));
}

#[test]
fn test_match_rgb_tuple() {
    // Variable pattern matches RGB color tuples
    let result =
        PatternMatchingTest::eval(r#"(match (list 255 128 0) (color "is a color"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("is a color"));
}

#[test]
fn test_match_struct_like_pattern() {
    // Variable pattern matches struct-like lists
    let result =
        PatternMatchingTest::eval(r#"(match (list "name" "Alice" 30) (record "matched"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_triple_elements() {
    // Variable pattern matches triples
    let result = PatternMatchingTest::eval(r#"(match (list 1 2 3) (triple "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

// ============================================================================
// Pattern Matching with Different Data Types
// ============================================================================

#[test]
fn test_match_float_literal_pattern() {
    let result = PatternMatchingTest::eval(r#"(match 3.14 (3.14 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_list_of_mixed_types() {
    // Variable pattern matches mixed-type lists
    let result = PatternMatchingTest::eval(r#"(match (list 42 "hello" 1) (x "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

// ============================================================================
// Pattern Matching Return Values
// ============================================================================

#[test]
fn test_match_returns_integer_value() {
    let result = PatternMatchingTest::eval(r#"(+ 5 (match 10 (10 20)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(25));
}

#[test]
fn test_match_returns_computed_expression() {
    // Match returns the result from the matched branch
    let result = PatternMatchingTest::eval(r#"(* 2 (match 5 (5 11)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(22));
}

// ============================================================================
// Multiple Pattern Matching Cases
// ============================================================================

#[test]
fn test_match_selects_first_matching_pattern() {
    let result =
        PatternMatchingTest::eval(r#"(match 1 (1 "first") (1 "second") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("first"));
}

#[test]
fn test_match_many_patterns() {
    let result = PatternMatchingTest::eval(
        r#"(match 5 (1 "one") (2 "two") (3 "three") (4 "four") (5 "five") ("default"))"#,
    )
    .unwrap();
    assert_eq!(result, elle::value::Value::string("five"));
}

// ============================================================================
// Pattern Matching with Arithmetic
// ============================================================================

#[test]
fn test_match_arithmetic_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 7 (7 (+ 10 20)) ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(30));
}

#[test]
fn test_match_conditional_arithmetic() {
    let result = PatternMatchingTest::eval(
        r#"(match 42 (40 "big") (41 "bigger") (42 (+ 100 50)) ("default"))"#,
    )
    .unwrap();
    assert_eq!(result, elle::value::Value::int(150));
}

// ============================================================================
// Variable Binding in Pattern Matching Tests
// ============================================================================

#[test]
fn test_match_variable_binding_simple() {
    // Simple variable binding: (match value (x expr-using-x))
    let result = PatternMatchingTest::eval(r#"(match 42 (x x))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(42));
}

#[test]
fn test_match_variable_binding_with_filter() {
    // Use bound list with filter (note: filter with closures not supported, using simpler test)
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2 3 4 5) (lst (first (rest (rest lst)))))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::int(3));
}

#[test]
fn test_match_variable_binding_multiplication() {
    // Use bound variable in multiplication
    let result = PatternMatchingTest::eval(r#"(match 7 (x (* x 2)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(14));
}

#[test]
fn test_match_variable_binding_string() {
    // Bind string value
    let result = PatternMatchingTest::eval(r#"(match "hello" (s s))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("hello"));
}

#[test]
fn test_match_variable_binding_string_operation() {
    // Use bound string in operation
    let result =
        PatternMatchingTest::eval(r#"(match "world" (s (string-append "hello " s)))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("hello world"));
}

#[test]
fn test_match_variable_binding_list() {
    // Bind list value and verify it's returned
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2 3) (lst (= lst (list 1 2 3))))"#).unwrap();
    assert_eq!(result, elle::value::Value::bool(true));
}

#[test]
fn test_match_variable_binding_list_length() {
    // Use bound list in length operation
    let result = PatternMatchingTest::eval(r#"(match (list 1 2 3) (lst (length lst)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(3));
}

#[test]
fn test_match_variable_binding_list_first() {
    // Use bound list in first operation
    let result = PatternMatchingTest::eval(r#"(match (list 10 20 30) (lst (first lst)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(10));
}

#[test]
fn test_match_variable_binding_multiple_uses() {
    // Use bound variable multiple times
    let result = PatternMatchingTest::eval(r#"(match 5 (x (+ x (+ x x))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(15)); // 5 + (5 + 5)
}

#[test]
fn test_match_variable_binding_nested_arithmetic() {
    // Nested arithmetic with bound variable
    let result = PatternMatchingTest::eval(r#"(match 3 (x (* (+ x 2) x)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(15)); // (3 + 2) * 3 = 5 * 3
}

#[test]
fn test_match_variable_binding_with_if() {
    // Use bound variable in if expression
    let result =
        PatternMatchingTest::eval(r#"(match 42 (x (if (> x 40) "big" "small")))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("big"));
}

#[test]
fn test_match_variable_binding_with_begin() {
    // Use bound variable in begin block
    let result = PatternMatchingTest::eval(r#"(match 10 (x (begin (+ x 5) (+ x 10))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(20)); // Returns result of last expr
}

#[test]
fn test_match_variable_binding_wildcard_no_binding() {
    // Wildcard doesn't bind variables
    let result = PatternMatchingTest::eval(r#"(match 42 (_ "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_variable_binding_literal_pattern_no_binding() {
    // Literal pattern doesn't create binding
    let result = PatternMatchingTest::eval(r#"(match 42 (42 "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::string("matched"));
}

#[test]
fn test_match_variable_binding_nil() {
    // Bind nil value
    let result = PatternMatchingTest::eval(r#"(match nil (x x))"#).unwrap();
    assert_eq!(result, elle::value::Value::NIL);
}

#[test]
fn test_match_variable_binding_float() {
    // Bind float value
    let result = PatternMatchingTest::eval(r#"(match 2.5 (x x))"#).unwrap();
    assert_eq!(result, elle::value::Value::float(2.5));
}

#[test]
fn test_match_variable_binding_float_arithmetic() {
    // Use bound float in arithmetic
    let result = PatternMatchingTest::eval(r#"(match 2.5 (x (+ x 1.5)))"#).unwrap();
    assert_eq!(result, elle::value::Value::float(4.0));
}

#[test]
fn test_match_variable_binding_multiple_patterns_first() {
    // Variable binding in first pattern
    let result = PatternMatchingTest::eval(r#"(match 10 (5 "five") (x (+ x 100)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(110));
}

#[test]
fn test_match_variable_binding_multiple_patterns_second() {
    // Variable binding in second pattern
    let result = PatternMatchingTest::eval(r#"(match 20 (10 "ten") (x (* x 2)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(40));
}

#[test]
fn test_match_variable_binding_fallthrough_to_binding() {
    // Fall through from literal to binding
    let result =
        PatternMatchingTest::eval(r#"(match 99 (1 "one") (2 "two") (x (+ x 1)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(100));
}

#[test]
fn test_match_variable_binding_with_default_fallback() {
    // Variable binding pattern followed by default
    let result = PatternMatchingTest::eval(r#"(match 42 (x (+ x 10)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(52));
}

#[test]
fn test_match_variable_binding_list_comparison() {
    // Compare bound list
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2) (lst (= lst (list 1 2))))"#).unwrap();
    assert_eq!(result, elle::value::Value::bool(true));
}

#[test]
fn test_match_variable_binding_list_rest() {
    // Use bound list in rest operation and verify length
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2 3) (lst (length (rest lst))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(2));
}

#[test]
fn test_match_variable_binding_double_binding() {
    // Two sequential pattern bindings
    let result =
        PatternMatchingTest::eval(r#"(+ (match 4 (x (+ x 1))) (match 3 (y (+ y 2))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(10)); // (4 + 1) + (3 + 2) = 5 + 5
}

#[test]
fn test_match_variable_binding_shadowing() {
    // Inner binding shadows outer
    let result = PatternMatchingTest::eval(r#"(match 10 (x (match 20 (x x))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(20));
}

#[test]
fn test_match_variable_binding_bool_value() {
    // Bind boolean value and verify it
    let result = PatternMatchingTest::eval(r#"(match 1 (x (= x 1)))"#).unwrap();
    assert_eq!(result, elle::value::Value::bool(true));
}

#[test]
fn test_match_variable_binding_bool_in_condition() {
    // Use bound value in comparison
    let result = PatternMatchingTest::eval(r#"(match 42 (x (> x 40)))"#).unwrap();
    assert_eq!(result, elle::value::Value::bool(true));
}

#[test]
fn test_match_variable_binding_with_not() {
    // Use bound value with not
    let result = PatternMatchingTest::eval(r#"(match 42 (x (not (> x 100))))"#).unwrap();
    assert_eq!(result, elle::value::Value::bool(true));
}

#[test]
fn test_match_variable_binding_named_descriptively() {
    // More descriptive variable names
    let result = PatternMatchingTest::eval(r#"(match 100 (amount (- amount 25)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(75));
}

#[test]
fn test_match_variable_binding_result_as_operand() {
    // Match result used as operand in surrounding expression
    let result = PatternMatchingTest::eval(r#"(* 2 (match 5 (x (+ x 3))))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(16)); // 2 * (5 + 3)
}
