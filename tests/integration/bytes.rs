// Bytes and blob type tests
//
// Tests for the immutable bytes and mutable blob types.

use crate::common::eval_source;

#[test]
fn test_bytes_constructor() {
    let result = eval_source("(bytes 72 101 108 108 111)").unwrap();
    assert!(result.is_bytes());
    let bytes = result.as_bytes().unwrap();
    assert_eq!(bytes.len(), 5);
    assert_eq!(bytes, &[72, 101, 108, 108, 111]);
}

#[test]
fn test_bytes_empty() {
    let result = eval_source("(bytes)").unwrap();
    assert!(result.is_bytes());
    let bytes = result.as_bytes().unwrap();
    assert_eq!(bytes.len(), 0);
}

#[test]
fn test_blob_constructor() {
    let result = eval_source("(blob 72 101 108 108 111)").unwrap();
    assert!(result.is_blob());
    let blob = result.as_blob().unwrap();
    assert_eq!(blob.borrow().len(), 5);
    assert_eq!(&blob.borrow()[..], &[72, 101, 108, 108, 111]);
}

#[test]
fn test_blob_empty() {
    let result = eval_source("(blob)").unwrap();
    assert!(result.is_blob());
    let blob = result.as_blob().unwrap();
    assert_eq!(blob.borrow().len(), 0);
}

#[test]
fn test_bytes_predicate() {
    let result = eval_source("(bytes? (bytes 1 2 3))").unwrap();
    assert!(result.is_bool());
    assert!(result.as_bool().unwrap());
}

#[test]
fn test_blob_predicate() {
    let result = eval_source("(blob? (blob 1 2 3))").unwrap();
    assert!(result.is_bool());
    assert!(result.as_bool().unwrap());
}

#[test]
fn test_string_to_bytes() {
    let result = eval_source(r#"(string->bytes "hello")"#).unwrap();
    assert!(result.is_bytes());
    let bytes = result.as_bytes().unwrap();
    assert_eq!(bytes, b"hello");
}

#[test]
fn test_string_to_blob() {
    let result = eval_source(r#"(string->blob "hello")"#).unwrap();
    assert!(result.is_blob());
    let blob = result.as_blob().unwrap();
    assert_eq!(&blob.borrow()[..], b"hello");
}

#[test]
fn test_bytes_to_string() {
    let result = eval_source(r#"(bytes->string (bytes 104 105))"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hi");
}

#[test]
fn test_blob_to_string() {
    let result = eval_source(r#"(blob->string (blob 104 105))"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hi");
}

#[test]
fn test_bytes_to_hex() {
    let result = eval_source("(bytes->hex (bytes 72 101 108))").unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "48656c");
}

#[test]
fn test_blob_to_hex() {
    let result = eval_source("(blob->hex (blob 72 101 108))").unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "48656c");
}

#[test]
fn test_bytes_length() {
    let result = eval_source("(length (bytes 1 2 3 4 5))").unwrap();
    assert!(result.is_int());
    assert_eq!(result.as_int().unwrap(), 5);
}

#[test]
fn test_blob_length() {
    let result = eval_source("(length (blob 1 2 3 4 5))").unwrap();
    assert!(result.is_int());
    assert_eq!(result.as_int().unwrap(), 5);
}

#[test]
fn test_bytes_get() {
    let result = eval_source("(get (bytes 72 101 108) 1)").unwrap();
    assert!(result.is_int());
    assert_eq!(result.as_int().unwrap(), 101);
}

#[test]
fn test_bytes_get_oob() {
    let result = eval_source("(get (bytes 72 101 108) 10)");
    assert!(result.is_err(), "get on bytes with OOB index should error");
}

#[test]
fn test_blob_get() {
    let result = eval_source("(get (blob 72 101 108) 1)").unwrap();
    assert!(result.is_int());
    assert_eq!(result.as_int().unwrap(), 101);
}

#[test]
fn test_blob_get_oob() {
    let result = eval_source("(get (blob 72 101 108) 10)");
    assert!(result.is_err(), "get on blob with OOB index should error");
}

#[test]
fn test_sha256_empty_string() {
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let result = eval_source(r#"(bytes->hex (crypto/sha256 ""))"#).unwrap();
    assert!(result.is_string());
    assert_eq!(
        result.with_string(|s| s.to_string()).unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_sha256_hello() {
    // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    let result = eval_source(r#"(bytes->hex (crypto/sha256 "hello"))"#).unwrap();
    assert!(result.is_string());
    assert_eq!(
        result.with_string(|s| s.to_string()).unwrap(),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_hmac_sha256() {
    let result = eval_source(r#"(bytes->hex (crypto/hmac-sha256 "key" "message"))"#).unwrap();
    assert!(result.is_string());
    // Just verify it produces a 64-character hex string (32 bytes)
    let hex = result.with_string(|s| s.to_string()).unwrap();
    assert_eq!(hex.len(), 64);
}

#[test]
fn test_uri_encode_simple() {
    let result = eval_source(r#"(uri-encode "hello")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hello");
}

#[test]
fn test_uri_encode_space() {
    let result = eval_source(r#"(uri-encode "hello world")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(
        result.with_string(|s| s.to_string()).unwrap(),
        "hello%20world"
    );
}

#[test]
fn test_uri_encode_special() {
    let result = eval_source(r#"(uri-encode "a/b")"#).unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "a%2Fb");
}

#[test]
fn test_sigv4_demo_runs() {
    // Run the demo and verify it completes without error
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("demos/aws-sigv4/sigv4.lisp")
        .output()
        .expect("failed to run sigv4 demo");
    assert!(
        output.status.success(),
        "sigv4 demo failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== Complete ==="), "demo did not complete");
    // Verify real crypto output (SHA-256 of empty string)
    assert!(
        stdout.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        "SHA-256 of empty string incorrect"
    );
}

#[test]
fn test_blob_push() {
    let result = eval_source("(let ((b (blob 1 2))) (push b 3) b)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[1, 2, 3]);
}

#[test]
fn test_blob_pop() {
    let result = eval_source("(let ((b (blob 1 2 3))) (pop b))").unwrap();
    assert_eq!(result.as_int().unwrap(), 3);
}

#[test]
fn test_blob_put() {
    let result = eval_source("(let ((b (blob 1 2 3))) (put b 1 99) (get b 1))").unwrap();
    assert_eq!(result.as_int().unwrap(), 99);
}

#[test]
fn test_slice_bytes() {
    let result = eval_source("(slice (bytes 1 2 3 4 5) 1 3)").unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), &[2, 3]);
}

#[test]
fn test_slice_blob() {
    let result = eval_source("(slice (blob 1 2 3 4 5) 1 3)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[2, 3]);
}

#[test]
fn test_append_bytes() {
    let result = eval_source("(append (bytes 1 2) (bytes 3 4))").unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), &[1, 2, 3, 4]);
}

#[test]
fn test_append_blob() {
    let result = eval_source("(let ((a (blob 1 2)) (b (blob 3 4))) (append a b) a)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[1, 2, 3, 4]);
}

#[test]
fn test_buffer_to_bytes() {
    let result = eval_source(r#"(buffer->bytes @"hello")"#).unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), b"hello");
}

#[test]
fn test_buffer_to_blob() {
    let result = eval_source(r#"(buffer->blob @"hello")"#).unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], b"hello");
}

#[test]
fn test_bytes_to_buffer() {
    let result = eval_source("(buffer->string (bytes->buffer (bytes 104 105)))").unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hi");
}

#[test]
fn test_blob_to_buffer() {
    let result = eval_source("(buffer->string (blob->buffer (blob 104 105)))").unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hi");
}

#[test]
fn test_each_over_bytes() {
    let result = eval_source(
        r#"
        (let ((sum 0))
          (each b (bytes 1 2 3)
            (set sum (+ sum b)))
          sum)
    "#,
    )
    .unwrap();
    assert_eq!(result.as_int().unwrap(), 6);
}

#[test]
fn test_each_over_blob() {
    let result = eval_source(
        r#"
        (let ((sum 0))
          (each b (blob 10 20 30)
            (set sum (+ sum b)))
          sum)
    "#,
    )
    .unwrap();
    assert_eq!(result.as_int().unwrap(), 60);
}

#[test]
fn test_map_over_tuple() {
    let result = eval_source("(map (fn (x) (+ x 1)) [1 2 3])").unwrap();
    // map returns a list
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_int().unwrap(), 2);
    assert_eq!(vec[1].as_int().unwrap(), 3);
    assert_eq!(vec[2].as_int().unwrap(), 4);
}

#[test]
fn test_map_over_bytes() {
    let result = eval_source("(map (fn (b) (* b 2)) (bytes 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_int().unwrap(), 2);
    assert_eq!(vec[1].as_int().unwrap(), 4);
    assert_eq!(vec[2].as_int().unwrap(), 6);
}
