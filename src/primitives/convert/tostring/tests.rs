//! audited: 2026-10-20
//! `string` over immutable bytes points into them; over a string or mutable
//! bytes it copies.
//!
//! docs/impl/values.md

use super::*;
use crate::primitives::ctx::with_test_ctx;

/// Where an immutable string's payload lies.
fn str_ptr(v: &Value) -> *const u8 {
    v.with_string(|s| s.as_ptr()).expect("an immutable string")
}

/// The trap: the right text at the wrong address passes a content check. The
/// address is the claim.
#[test]
fn string_over_immutable_bytes_points_into_them() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes(b"hello".to_vec());
        let (sig, s) = prim_to_string(ctx, &[src]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(s.with_string(str::to_string).unwrap(), "hello");
        assert_eq!(
            str_ptr(&s),
            src.as_bytes().unwrap().as_ptr(),
            "the string must lie over the bytes, not in a copy"
        );
    });
}

/// Bytes that are not UTF-8 answer an encoding error, and no string.
#[test]
fn string_over_bytes_that_are_not_utf8_is_an_encoding_error() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes(vec![0xff, 0x61]);
        let (sig, err) = prim_to_string(ctx, &[src]);
        assert_eq!(sig, SIG_ERROR);
        let kind = err
            .as_struct()
            .and_then(|s| {
                crate::value::sorted_struct_get(s, &crate::value::heap::TableKey::keyword("error"))
                    .copied()
            })
            .expect("an error struct");
        assert_eq!(kind, Value::keyword("encoding-error"));
    });
}

/// `string` of a string is a copy: the declaration oracle for `Fresh` reads
/// `region_of(result)`, and the argument lives in the caller's region.
#[test]
fn string_of_a_string_is_a_copy() {
    with_test_ctx(|ctx| {
        let src = ctx.string("hello");
        let (sig, s) = prim_to_string(ctx, &[src]);
        assert_eq!(sig, SIG_OK);
        assert_ne!(str_ptr(&s), str_ptr(&src), "a string copies a string");
    });
}

/// A `@bytes` payload can move under a view, so the string copies it.
#[test]
fn string_over_mutable_bytes_is_a_copy() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes_mut(b"hello".to_vec());
        let (sig, s) = prim_to_string(ctx, &[src]);
        assert_eq!(sig, SIG_OK);
        let src_ptr = src.as_bytes_mut().unwrap().borrow().as_ptr();
        assert_ne!(str_ptr(&s), src_ptr, "a string copies @bytes");
    });
}
