// Integration tests for new/refactored primitive modules
// Tests that require Rust APIs or check error message content
use crate::common::eval_source;
use elle::Value;

// === Read primitives ===

#[test]
fn test_read_list() {
    // read should parse a list form
    let result = eval_source("(read \"(+ 1 2)\")").unwrap();
    assert!(result.as_cons().is_some(), "Expected a cons cell (list)");
}

#[test]
fn test_read_type_error() {
    let result = eval_source("(read 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_read_all_multiple_forms() {
    let result = eval_source("(read-all \"1 2 3\")").unwrap();
    // Should return a list of three values
    let first = result.as_cons().expect("should be a list");
    assert_eq!(first.first, Value::int(1));
}

#[test]
fn test_read_all_type_error() {
    let result = eval_source("(read-all 42)");
    assert!(result.is_err());
}

// === Conversion primitives ===

#[test]
fn test_integer_type_error() {
    let result = eval_source("(integer true)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_number_to_string_type_error() {
    let result = eval_source("(number->string \"hello\")");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_keyword_to_string_type_error() {
    let result = eval_source("(keyword->string 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_symbol_to_string_type_error() {
    let result = eval_source("(symbol->string 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_path_join_type_error() {
    let result = eval_source("(path/join 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_path_cwd() {
    let result = eval_source("(path/cwd)").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert!(!s.is_empty());
}

#[test]
fn test_path_absolute() {
    let result = eval_source("(path/absolute \"src\")").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert!(s.starts_with('/'), "absolute path should start with /");
}

#[test]
fn test_path_canonicalize_dot() {
    let result = eval_source("(path/canonicalize \".\")").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert!(s.starts_with('/'));
}

#[test]
fn test_path_canonicalize_nonexistent() {
    let result = eval_source("(path/canonicalize \"/nonexistent/path/xyz\")");
    assert!(result.is_err());
}

#[test]
fn test_string_from_list() {
    let result = eval_source("(string (list 1 2 3))").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert_eq!(s, "(1 2 3)");
}

#[test]
fn test_string_from_array() {
    let result = eval_source("(string @[1 2 3])").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert_eq!(s, "[1, 2, 3]");
}

#[test]
fn test_string_from_float() {
    // Float formatting may vary, just check it's a string
    let result = eval_source("(string 3.14)").unwrap();
    assert!(result.is_string());
}

#[test]
fn test_number_to_string_float() {
    let result = eval_source("(number->string 3.14)").unwrap();
    assert!(result.is_string());
}

#[test]
fn test_string_from_keyword() {
    let result = eval_source("(string :hello)").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert_eq!(s, ":hello");
}

#[test]
fn test_string_from_empty_list() {
    // Empty list should have some string representation
    let result = eval_source("(string (list))").unwrap();
    assert!(result.is_string());
}

#[test]
fn test_take_negative_count_errors() {
    let result = eval_source("(take -1 (list 1 2 3))");
    assert!(result.is_err(), "take with negative count should error");
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_drop_negative_count_errors() {
    let result = eval_source("(drop -1 (list 1 2 3))");
    assert!(result.is_err(), "drop with negative count should error");
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_bit_and_nan_errors() {
    let result = eval_source("(bit/and (sqrt -1.0) 1)");
    assert!(result.is_err(), "NaN should error in bitwise ops");
    assert!(result.unwrap_err().contains("non-finite"));
}

#[test]
fn test_bit_and_inf_errors() {
    let result = eval_source("(bit/and (exp 1000.0) 1)");
    assert!(result.is_err(), "infinity should error in bitwise ops");
    assert!(result.unwrap_err().contains("non-finite"));
}

#[test]
fn test_first_non_sequence_errors() {
    let result = eval_source("(first 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_rest_non_sequence_errors() {
    let result = eval_source("(rest 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_reverse_non_sequence_errors() {
    let result = eval_source("(reverse 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}
