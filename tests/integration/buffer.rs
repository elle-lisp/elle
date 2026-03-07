// Buffer type tests
//
// Tests for the mutable buffer type (@"..." literals and operations).
// Most tests migrated to tests/elle/buffer.lisp.
// This file retains tests that require byte-level inspection or display formatting.

use crate::common::eval_source;

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
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hello");
}

#[test]
fn test_buffer_to_string_empty() {
    let result = eval_source(r#"(buffer->string @"")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "");
}

#[test]
fn test_buffer_get() {
    // Buffer get returns character as string, not byte
    let result = eval_source(r#"(get @"hello" 0)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "h");
}

#[test]
fn test_buffer_get_middle() {
    let result = eval_source(r#"(get @"hello" 2)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "l");
}

#[test]
fn test_buffer_get_last() {
    let result = eval_source(r#"(get @"hello" 4)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "o");
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

#[test]
fn test_buffer_get_unicode() {
    // Buffer with UTF-8 multi-byte character
    let result = eval_source(r#"(get @"café" 3)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "é");
}

#[test]
fn test_buffer_get_unicode_index() {
    // Character indexing, not byte indexing
    let result = eval_source(r#"(get @"café" 0)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "c");
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
    let tuple = result.as_tuple().unwrap();
    assert_eq!(tuple.len(), 3);
    assert_eq!(tuple[0].with_string(|s| s.to_string()).unwrap(), "a");
    assert_eq!(tuple[1].with_string(|s| s.to_string()).unwrap(), "b");
    assert_eq!(tuple[2].with_string(|s| s.to_string()).unwrap(), "c");
}

#[test]
fn test_buffer_replace() {
    let result = eval_source(r#"(string/replace @"hello" "l" "L")"#).unwrap();
    assert!(result.is_buffer());
    let buf = result.as_buffer().unwrap();
    assert_eq!(&buf.borrow()[..], b"heLLo");
}
