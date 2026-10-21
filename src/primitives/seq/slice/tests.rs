// audited: 2026-10-20
//! A slice of an immutable string or bytes points into its source; a slice of
//! a mutable one is a copy.
//!
//! docs/impl/values.md

use super::*;
use crate::primitives::ctx::with_test_ctx;

/// Where an immutable bytes value's payload lies.
fn bytes_ptr(v: &Value) -> *const u8 {
    v.as_bytes().expect("immutable bytes").as_ptr()
}

/// Where an immutable string's payload lies.
fn str_ptr(v: &Value) -> *const u8 {
    v.with_string(|s| s.as_ptr()).expect("an immutable string")
}

/// The trap: a slice that copies answers the right bytes at the wrong address,
/// so the content assertion alone proves nothing. The address is the claim.
#[test]
fn a_slice_of_immutable_bytes_points_into_its_source() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes(b"hello world".to_vec());
        let v = seq_slice(&src, 2, 5, ctx).expect("a slice");
        assert_eq!(v.as_bytes().unwrap(), b"llo");
        assert_eq!(
            bytes_ptr(&v),
            unsafe { bytes_ptr(&src).add(2) },
            "the slice must lie two bytes into the source, not in a copy"
        );
    });
}

/// `slice` counts graphemes on a string, so the view starts at the byte the
/// first grapheme in range starts at: one byte in for `h`, three for `hé`.
#[test]
fn a_slice_of_a_string_points_into_its_source_at_the_grapheme_boundary() {
    with_test_ctx(|ctx| {
        let src = ctx.string("héllo");
        let v = seq_slice(&src, 1, 3, ctx).expect("a slice");
        assert_eq!(v.with_string(str::to_string).unwrap(), "él");
        assert_eq!(
            str_ptr(&v),
            unsafe { str_ptr(&src).add(1) },
            "the slice must start at the second grapheme's first byte"
        );
        let rest = seq_slice(&src, 2, 5, ctx).expect("a slice");
        assert_eq!(rest.with_string(str::to_string).unwrap(), "llo");
        assert_eq!(str_ptr(&rest), unsafe { str_ptr(&src).add(3) });
    });
}

/// An empty slice has no backing to share: a dangling pointer, length zero.
#[test]
fn an_empty_slice_shares_nothing() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes(b"hello".to_vec());
        let v = seq_slice(&src, 3, 3, ctx).expect("a slice");
        assert!(v.as_bytes().unwrap().is_empty());
        let s = seq_slice(&ctx.string("hello"), 5, 5, ctx).expect("a slice");
        assert_eq!(s.with_string(str::to_string).unwrap(), "");
    });
}

/// A mutable value keeps its bytes in a `Vec` another holder can grow, so a
/// view into it would dangle on the first push. The slice copies.
#[test]
fn a_slice_of_mutable_bytes_is_a_copy() {
    with_test_ctx(|ctx| {
        let src = ctx.bytes_mut(b"hello world".to_vec());
        let v = seq_slice(&src, 2, 5, ctx).expect("a slice");
        let src_ptr = src.as_bytes_mut().unwrap().borrow().as_ptr();
        let v_ref = v.as_bytes_mut().expect("a slice of @bytes is @bytes");
        let v_bytes = v_ref.borrow();
        assert_eq!(&v_bytes[..], b"llo");
        assert_ne!(
            v_bytes.as_ptr(),
            unsafe { src_ptr.add(2) },
            "a slice of @bytes must own its bytes"
        );
    });
}
