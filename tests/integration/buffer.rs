// Buffer type tests
//
// Tests for the mutable buffer type (@"..." literals and operations).

use crate::common::eval_source;
use elle::Value;

#[test]
fn test_buffer_literal() {
    let result = eval_source(r#"@"hello""#).unwrap();
    assert!(result.is_buffer());
}

#[test]
fn test_buffer_empty() {
    let result = eval_source(r#"@"""#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(buf.borrow().len(), 0);
}

#[test]
fn test_buffer_display() {
    let result = eval_source(r#"@"hello""#).unwrap();
    let display = format!("{}", result);
    assert_eq!(display, r#"@"hello""#);
}

#[test]
fn test_buffer_display_empty() {
    let result = eval_source(r#"@"""#).unwrap();
    let display = format!("{}", result);
    assert_eq!(display, r#"@"""#);
}

#[test]
fn test_buffer_constructor() {
    let result = eval_source("(buffer)").unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(buf.borrow().len(), 0);
}

#[test]
fn test_buffer_constructor_with_bytes() {
    let result = eval_source("(buffer 72 101 108 108 111)").unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 5);
    assert_eq!(borrowed[0], 72); // 'H'
    assert_eq!(borrowed[1], 101); // 'e'
}

#[test]
fn test_string_to_buffer() {
    let result = eval_source(r#"(string->buffer "hello")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 5);
    assert_eq!(&borrowed[..], b"hello");
}

#[test]
fn test_buffer_to_string() {
    let result = eval_source(r#"(buffer->string @"hello")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.as_string().unwrap(), "hello");
}

#[test]
fn test_buffer_to_string_empty() {
    let result = eval_source(r#"(buffer->string @"")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.as_string().unwrap(), "");
}

#[test]
fn test_buffer_predicate() {
    let result = eval_source(r#"(buffer? @"hello")"#).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_buffer_predicate_false() {
    let result = eval_source(r#"(buffer? "hello")"#).unwrap();
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_buffer_get() {
    // Buffer get returns character as string, not byte
    let result = eval_source(r#"(get @"hello" 0)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "h");
}

#[test]
fn test_buffer_get_middle() {
    let result = eval_source(r#"(get @"hello" 2)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "l");
}

#[test]
fn test_buffer_get_last() {
    let result = eval_source(r#"(get @"hello" 4)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "o");
}

#[test]
fn test_buffer_get_out_of_bounds() {
    let result = eval_source(r#"(get @"hello" 100)"#).unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_buffer_get_with_default() {
    let result = eval_source(r#"(get @"hello" 100 99)"#).unwrap();
    assert_eq!(result.as_int(), Some(99));
}

#[test]
fn test_buffer_put() {
    let result = eval_source(r#"(begin (var b @"hello") (put b 0 88) b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed[0], 88); // 'X'
    assert_eq!(borrowed[1], 101); // 'e'
}

#[test]
fn test_buffer_push() {
    let result = eval_source(r#"(begin (var b @"hi") (push b 33) b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[2], 33); // '!'
}

#[test]
fn test_buffer_pop() {
    let result = eval_source(r#"(begin (var b @"hi") (pop b))"#).unwrap();
    assert_eq!(result.as_int(), Some(105)); // 'i'
}

#[test]
fn test_buffer_pop_empty() {
    let result = eval_source(r#"(begin (var b @"") (pop b))"#);
    // Should error on empty pop
    assert!(result.is_err());
}

#[test]
fn test_buffer_length() {
    let result = eval_source(r#"(length @"hello")"#).unwrap();
    assert_eq!(result.as_int(), Some(5));
}

#[test]
fn test_buffer_length_empty() {
    let result = eval_source(r#"(length @"")"#).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_buffer_empty_predicate() {
    let result = eval_source(r#"(empty? @"")"#).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_buffer_empty_predicate_false() {
    let result = eval_source(r#"(empty? @"hello")"#).unwrap();
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_buffer_append() {
    let result = eval_source(r#"(begin (var b @"hello") (append b @" world") b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 11);
    assert_eq!(&borrowed[..], b"hello world");
}

#[test]
fn test_buffer_concat() {
    let result = eval_source(r#"(concat @"hello" @" world")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 11);
    assert_eq!(&borrowed[..], b"hello world");
}

#[test]
fn test_buffer_roundtrip() {
    let result = eval_source(r#"(buffer->string (string->buffer "hello"))"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "hello");
}

#[test]
fn test_buffer_literal_roundtrip() {
    let result = eval_source(r#"(buffer->string @"hello")"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "hello");
}

#[test]
fn test_buffer_insert() {
    let result = eval_source(r#"(begin (var b @"hllo") (insert b 1 101) b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(&borrowed[..], b"hello");
}

#[test]
fn test_buffer_remove() {
    let result = eval_source(r#"(begin (var b @"hello") (remove b 1) b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(&borrowed[..], b"hllo");
}

#[test]
fn test_buffer_remove_multiple() {
    let result = eval_source(r#"(begin (var b @"hello") (remove b 1 2) b)"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(&borrowed[..], b"hlo");
}

#[test]
fn test_buffer_popn() {
    // "hello" = [104, 101, 108, 108, 111]
    // popn 2 removes last 2 bytes: [108, 111] ('l', 'o')
    let result = eval_source(r#"(begin (var b @"hello") (popn b 2))"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    let borrowed = buf.borrow();
    assert_eq!(borrowed.len(), 2);
    assert_eq!(borrowed[0], 108); // 'l'
    assert_eq!(borrowed[1], 111); // 'o'
}

// ============================================================================
// Buffer put out-of-bounds errors
// ============================================================================

#[test]
fn test_buffer_put_out_of_bounds_errors() {
    let result = eval_source(r#"(put @"hello" 10 88)"#);
    assert!(result.is_err());
}

#[test]
fn test_buffer_put_negative_index_errors() {
    let result = eval_source(r#"(put @"hello" -1 88)"#);
    assert!(result.is_err());
}

#[test]
fn test_buffer_put_empty_errors() {
    let result = eval_source(r#"(put @"" 0 88)"#);
    assert!(result.is_err());
}

// ============================================================================
// Buffer get returns character (string), not byte
// ============================================================================

#[test]
fn test_buffer_get_unicode() {
    // Buffer with UTF-8 multi-byte character
    let result = eval_source(r#"(get @"café" 3)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "é");
}

#[test]
fn test_buffer_get_unicode_index() {
    // Character indexing, not byte indexing
    let result = eval_source(r#"(get @"café" 0)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "c");
}

// ============================================================================
// String operations on buffers
// ============================================================================

#[test]
fn test_buffer_contains() {
    let result = eval_source(r#"(string/contains? @"hello world" "world")"#).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_buffer_contains_false() {
    let result = eval_source(r#"(string/contains? @"hello" "xyz")"#).unwrap();
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_buffer_starts_with() {
    let result = eval_source(r#"(string/starts-with? @"hello" "he")"#).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_buffer_starts_with_false() {
    let result = eval_source(r#"(string/starts-with? @"hello" "lo")"#).unwrap();
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_buffer_ends_with() {
    let result = eval_source(r#"(string/ends-with? @"hello" "lo")"#).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_buffer_ends_with_false() {
    let result = eval_source(r#"(string/ends-with? @"hello" "he")"#).unwrap();
    assert_eq!(result, Value::bool(false));
}

#[test]
fn test_buffer_index() {
    let result = eval_source(r#"(string/index @"hello" "l")"#).unwrap();
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_buffer_index_not_found() {
    let result = eval_source(r#"(string/index @"hello" "z")"#).unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_buffer_substring() {
    let result = eval_source(r#"(substring @"hello" 1 4)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "ell");
}

#[test]
fn test_buffer_upcase() {
    let result = eval_source(r#"(string/upcase @"hello")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(&buf.borrow()[..], b"HELLO");
}

#[test]
fn test_buffer_downcase() {
    let result = eval_source(r#"(string/downcase @"HELLO")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(&buf.borrow()[..], b"hello");
}

#[test]
fn test_buffer_trim() {
    let result = eval_source(r#"(string/trim @"  hello  ")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(&buf.borrow()[..], b"hello");
}

#[test]
fn test_buffer_split() {
    let result = eval_source(r#"(string/split @"a,b,c" ",")"#).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_string().unwrap(), "a");
    assert_eq!(vec[1].as_string().unwrap(), "b");
    assert_eq!(vec[2].as_string().unwrap(), "c");
}

#[test]
fn test_buffer_replace() {
    let result = eval_source(r#"(string/replace @"hello" "l" "L")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(&buf.borrow()[..], b"heLLo");
}

#[test]
fn test_buffer_char_at() {
    let result = eval_source(r#"(string/char-at @"hello" 1)"#).unwrap();
    assert_eq!(result.as_string().unwrap(), "e");
}

// ============================================================================
// concat on lists
// ============================================================================

#[test]
fn test_concat_lists() {
    let result = eval_source("(concat (list 1 2) (list 3 4))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_concat_empty_lists() {
    let result = eval_source("(concat (list) (list 1 2))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_concat_lists_original_unchanged() {
    let result = eval_source(
        r#"(let ((a (list 1 2)))
             (let ((b (concat a (list 3 4))))
               (list (length a) (length b))))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::int(2)); // Original unchanged
    assert_eq!(list[1], Value::int(4)); // Concatenated has all
}
