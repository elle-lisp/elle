// End-to-end FFI integration tests.
// These test the full pipeline: Elle source → compiler → VM → libffi → C.
// Most tests have been migrated to tests/elle/ffi.lisp.
// This file retains only tests that require Rust-level assertions for:
// - Epsilon tolerance comparisons (.as_float())
// - Error message content checking (.contains())

use crate::common::eval_source;
use elle::Value;

// ── Epsilon tolerance tests ─────────────────────────────────────────

#[test]
fn test_ffi_read_write_double() {
    let result = eval_source(
        "
        (def ptr (ffi/malloc 8))
        (ffi/write ptr :double 1.234)
        (def val (ffi/read ptr :double))
        (ffi/free ptr)
        val
    ",
    );
    assert_eq!(result.unwrap(), Value::float(1.234));
}

#[test]
fn test_ffi_struct_read_write_roundtrip() {
    let result = eval_source(
        r#"
        (def st (ffi/struct @[:i32 :double]))
        (def buf (ffi/malloc (ffi/size st)))
        (ffi/write buf st @[42 3.14])
        (def vals (ffi/read buf st))
        (ffi/free buf)
        vals
    "#,
    );
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0].as_int(), Some(42));
    let expected = 314.0 / 100.0;
    assert!((arr[1].as_float().unwrap() - expected).abs() < 1e-10);
}

#[test]
fn test_ffi_struct_with_all_numeric_types() {
    let result = eval_source(
        r#"
        (def st (ffi/struct @[:i8 :u8 :i16 :u16 :i32 :u32 :i64 :u64 :float :double]))
        (def buf (ffi/malloc (ffi/size st)))
        (ffi/write buf st @[-1 255 -1000 60000 -100000 3000000000 -999999999 999999999 1.5 2.5])
        (def vals (ffi/read buf st))
        (ffi/free buf)
        vals
    "#,
    );
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0].as_int(), Some(-1)); // i8
    assert_eq!(arr[1].as_int(), Some(255)); // u8
    assert_eq!(arr[2].as_int(), Some(-1000)); // i16
    assert_eq!(arr[3].as_int(), Some(60000)); // u16
    assert_eq!(arr[4].as_int(), Some(-100000)); // i32
    assert_eq!(arr[5].as_int(), Some(3000000000)); // u32
    assert_eq!(arr[6].as_int(), Some(-999999999)); // i64
    assert_eq!(arr[7].as_int(), Some(999999999)); // u64
    assert_eq!(arr[8].as_float(), Some(1.5)); // float
    assert_eq!(arr[9].as_float(), Some(2.5)); // double
}

// ── Error message checking tests ────────────────────────────────────

#[test]
fn test_ffi_double_free_error() {
    let result = eval_source(
        "(let ((ptr (ffi/malloc 8)))
            (ffi/free ptr)
            (ffi/free ptr))",
    );
    assert!(result.is_err(), "Double free should signal an error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("already been freed") || err.contains("double-free"),
        "Error should mention double-free: {}",
        err
    );
}

#[test]
fn test_ffi_use_after_free_read_error() {
    let result = eval_source(
        "(let ((ptr (ffi/malloc 8)))
            (ffi/write ptr :int 42)
            (ffi/free ptr)
            (ffi/read ptr :int))",
    );
    assert!(
        result.is_err(),
        "Use-after-free read should signal an error"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("freed"),
        "Error should mention freed pointer: {}",
        err
    );
}

#[test]
fn test_ffi_use_after_free_write_error() {
    let result = eval_source(
        "(let ((ptr (ffi/malloc 8)))
            (ffi/free ptr)
            (ffi/write ptr :int 99))",
    );
    assert!(
        result.is_err(),
        "Use-after-free write should signal an error"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("freed"),
        "Error should mention freed pointer: {}",
        err
    );
}
