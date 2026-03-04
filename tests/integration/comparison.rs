use crate::common::eval_source_bare as eval_source;
use elle::Value;

// =============================================================================
// String comparison — basic
// =============================================================================

#[test]
fn string_lt() {
    assert_eq!(eval_source(r#"(< "a" "b")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(< "b" "a")"#).unwrap(), Value::FALSE);
    assert_eq!(eval_source(r#"(< "a" "a")"#).unwrap(), Value::FALSE);
}

#[test]
fn string_gt() {
    assert_eq!(eval_source(r#"(> "b" "a")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(> "a" "b")"#).unwrap(), Value::FALSE);
    assert_eq!(eval_source(r#"(> "a" "a")"#).unwrap(), Value::FALSE);
}

#[test]
fn string_le() {
    assert_eq!(eval_source(r#"(<= "a" "b")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(<= "a" "a")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(<= "b" "a")"#).unwrap(), Value::FALSE);
}

#[test]
fn string_ge() {
    assert_eq!(eval_source(r#"(>= "b" "a")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(>= "a" "a")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(>= "a" "b")"#).unwrap(), Value::FALSE);
}

// =============================================================================
// Keyword comparison
// =============================================================================

#[test]
fn keyword_lt() {
    assert_eq!(eval_source("(< :apple :banana)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(< :banana :apple)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(< :apple :apple)").unwrap(), Value::FALSE);
}

#[test]
fn keyword_gt() {
    assert_eq!(eval_source("(> :banana :apple)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(> :apple :banana)").unwrap(), Value::FALSE);
}

#[test]
fn keyword_le() {
    assert_eq!(eval_source("(<= :apple :banana)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(<= :apple :apple)").unwrap(), Value::TRUE);
}

#[test]
fn keyword_ge() {
    assert_eq!(eval_source("(>= :banana :apple)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(>= :apple :apple)").unwrap(), Value::TRUE);
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn empty_string_comparison() {
    assert_eq!(eval_source(r#"(< "" "a")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(< "" "")"#).unwrap(), Value::FALSE);
    assert_eq!(eval_source(r#"(<= "" "")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(> "a" "")"#).unwrap(), Value::TRUE);
}

#[test]
fn unicode_string_comparison() {
    // Lexicographic by byte (Rust's str::cmp is byte-order)
    assert_eq!(eval_source(r#"(< "α" "β")"#).unwrap(), Value::TRUE);
}

#[test]
fn string_prefix_comparison() {
    assert_eq!(eval_source(r#"(< "abc" "abcd")"#).unwrap(), Value::TRUE);
    assert_eq!(eval_source(r#"(> "abcd" "abc")"#).unwrap(), Value::TRUE);
}

// =============================================================================
// Mixed-type errors
// =============================================================================

#[test]
fn mixed_type_error_string_int() {
    let err = eval_source(r#"(< "a" 1)"#).unwrap_err();
    assert!(err.contains("type-error"), "got: {err}");
}

#[test]
fn mixed_type_error_string_keyword() {
    let err = eval_source(r#"(< "a" :b)"#).unwrap_err();
    assert!(err.contains("type-error"), "got: {err}");
}

#[test]
fn mixed_type_error_keyword_int() {
    let err = eval_source(r#"(< :a 1)"#).unwrap_err();
    assert!(err.contains("type-error"), "got: {err}");
}

#[test]
fn buffer_comparison_rejected() {
    let err = eval_source(r#"(< @"a" @"b")"#).unwrap_err();
    assert!(err.contains("type-error"), "got: {err}");
}

// =============================================================================
// Existing numeric behavior preserved
// =============================================================================

#[test]
fn numeric_comparison_unchanged() {
    assert_eq!(eval_source("(< 1 2)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(< 2 1)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(> 2 1)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(<= 1 1)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(>= 1 1)").unwrap(), Value::TRUE);
}
