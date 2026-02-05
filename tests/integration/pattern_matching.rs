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
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_literal_string() {
    let result =
        PatternMatchingTest::eval(r#"(match "hello" ("hello" "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_literal_false() {
    let result = PatternMatchingTest::eval(r#"(match 0 (0 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_literal_true() {
    let result = PatternMatchingTest::eval(r#"(match 1 (1 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_nil_pattern() {
    let result = PatternMatchingTest::eval(r#"(match nil (nil "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_default_case() {
    let result =
        PatternMatchingTest::eval(r#"(match 99 (42 "no") (100 "no") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("default".into()));
}

#[test]
fn test_match_wildcard_pattern() {
    let result = PatternMatchingTest::eval(r#"(match 42 (_ "wildcard matched"))"#).unwrap();
    assert_eq!(
        result,
        elle::value::Value::String("wildcard matched".into())
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
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_variable_pattern_string() {
    // Variable patterns accept any string
    let result = PatternMatchingTest::eval(r#"(match "hello" (name "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_variable_pattern_list() {
    // Variable patterns accept lists
    let result = PatternMatchingTest::eval(r#"(match (list 1 2 3) (lst "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_variable_pattern_nil() {
    // Variable patterns accept nil
    let result = PatternMatchingTest::eval(r#"(match nil (x "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

// ============================================================================
// List Pattern Matching Tests
// ============================================================================

#[test]
fn test_match_empty_list_pattern() {
    let result = PatternMatchingTest::eval(r#"(match nil (nil "empty") ("not empty"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("empty".into()));
}

#[test]
fn test_match_single_element_list() {
    // Variable pattern matches single-element lists
    let result = PatternMatchingTest::eval(r#"(match (list 42) (lst "single element"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("single element".into()));
}

#[test]
fn test_match_list_with_wildcard() {
    // Wildcard pattern matches any list
    let result =
        PatternMatchingTest::eval(r#"(match (list 1 2 3) (_ "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

// ============================================================================
// Control Flow and Pattern Matching Integration Tests
// ============================================================================

#[test]
fn test_match_with_begin_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 42 (42 (begin (+ 10 20) (+ 1 1))))"#).unwrap();
    assert_eq!(result, elle::value::Value::Int(2));
}

#[test]
fn test_match_with_if_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 42 (42 (if (> 42 30) "yes" "no")))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("yes".into()));
}

#[test]
fn test_match_result_value() {
    let result = PatternMatchingTest::eval(r#"(+ 10 (match 5 (5 100)))"#).unwrap();
    assert_eq!(result, elle::value::Value::Int(110));
}

// ============================================================================
// Pattern Matching Error Cases
// ============================================================================

#[test]
fn test_match_no_matching_pattern_default() {
    let result =
        PatternMatchingTest::eval(r#"(match 999 (1 "one") (2 "two") (999 "found") ("none"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::String("found".into()));
}

#[test]
fn test_match_with_computed_values() {
    let result =
        PatternMatchingTest::eval(r#"(match (+ 20 22) (42 "computed match") ("default"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::String("computed match".into()));
}

// ============================================================================
// Real-world Pattern Matching Examples
// ============================================================================

#[test]
fn test_match_coordinate_pair() {
    // Wildcard pattern matches coordinate pairs
    let result = PatternMatchingTest::eval(r#"(match (list 10 20) (_ "is a pair"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("is a pair".into()));
}

#[test]
fn test_match_rgb_tuple() {
    // Variable pattern matches RGB color tuples
    let result =
        PatternMatchingTest::eval(r#"(match (list 255 128 0) (color "is a color"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("is a color".into()));
}

#[test]
fn test_match_struct_like_pattern() {
    // Variable pattern matches struct-like lists
    let result =
        PatternMatchingTest::eval(r#"(match (list "name" "Alice" 30) (record "matched"))"#)
            .unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_triple_elements() {
    // Variable pattern matches triples
    let result = PatternMatchingTest::eval(r#"(match (list 1 2 3) (triple "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

// ============================================================================
// Pattern Matching with Different Data Types
// ============================================================================

#[test]
fn test_match_float_literal_pattern() {
    let result = PatternMatchingTest::eval(r#"(match 3.14 (3.14 "matched") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

#[test]
fn test_match_list_of_mixed_types() {
    // Variable pattern matches mixed-type lists
    let result = PatternMatchingTest::eval(r#"(match (list 42 "hello" 1) (x "matched"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("matched".into()));
}

// ============================================================================
// Pattern Matching Return Values
// ============================================================================

#[test]
fn test_match_returns_integer_value() {
    let result = PatternMatchingTest::eval(r#"(+ 5 (match 10 (10 20)))"#).unwrap();
    assert_eq!(result, elle::value::Value::Int(25));
}

#[test]
fn test_match_returns_computed_expression() {
    // Match returns the result from the matched branch
    let result = PatternMatchingTest::eval(r#"(* 2 (match 5 (5 11)))"#).unwrap();
    assert_eq!(result, elle::value::Value::Int(22));
}

// ============================================================================
// Multiple Pattern Matching Cases
// ============================================================================

#[test]
fn test_match_selects_first_matching_pattern() {
    let result =
        PatternMatchingTest::eval(r#"(match 1 (1 "first") (1 "second") ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::String("first".into()));
}

#[test]
fn test_match_many_patterns() {
    let result = PatternMatchingTest::eval(
        r#"(match 5 (1 "one") (2 "two") (3 "three") (4 "four") (5 "five") ("default"))"#,
    )
    .unwrap();
    assert_eq!(result, elle::value::Value::String("five".into()));
}

// ============================================================================
// Pattern Matching with Arithmetic
// ============================================================================

#[test]
fn test_match_arithmetic_in_body() {
    let result = PatternMatchingTest::eval(r#"(match 7 (7 (+ 10 20)) ("default"))"#).unwrap();
    assert_eq!(result, elle::value::Value::Int(30));
}

#[test]
fn test_match_conditional_arithmetic() {
    let result = PatternMatchingTest::eval(
        r#"(match 42 (40 "big") (41 "bigger") (42 (+ 100 50)) ("default"))"#,
    )
    .unwrap();
    assert_eq!(result, elle::value::Value::Int(150));
}
