// audited: 2026-10-13
//! Preparing a CIF, and calling C through `ffi_call`: arity, arguments and return values.
//!
//! docs/ffi.md

use super::*;
use crate::ffi::types::{CallingConvention, Signature};

fn signature(ret: TypeDesc, args: Vec<TypeDesc>, fixed_args: Option<usize>) -> Signature {
    Signature {
        convention: CallingConvention::Default,
        ret,
        args,
        fixed_args,
    }
}

#[test]
fn test_prepare_cif() {
    let sig = signature(TypeDesc::I32, vec![TypeDesc::I32], None);
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_prepare_cif_no_args() {
    let sig = signature(TypeDesc::Void, vec![], None);
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_prepare_variadic_cif() {
    let sig = signature(
        TypeDesc::I32,
        vec![TypeDesc::Ptr, TypeDesc::Size, TypeDesc::Ptr, TypeDesc::I32],
        Some(3),
    );
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_arity_check() {
    let sig = signature(TypeDesc::I32, vec![TypeDesc::I32], None);
    let cif = prepare_cif(&sig);
    // Wrong number of args
    let result = crate::primitives::ctx::with_test_ctx(|ctx| unsafe {
        ffi_call(std::ptr::null(), &[], &sig, &cif, ctx)
    });
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn test_call_abs() {
    extern "C" {
        fn abs(n: std::ffi::c_int) -> std::ffi::c_int;
    }
    let sig = signature(TypeDesc::Int, vec![TypeDesc::Int], None);
    let cif = prepare_cif(&sig);
    let result = crate::primitives::ctx::with_test_ctx(|ctx| unsafe {
        ffi_call(abs as *const c_void, &[Value::int(-42)], &sig, &cif, ctx)
    });
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_int(), Some(42));
}

#[cfg(unix)]
#[test]
fn test_call_strlen() {
    crate::value::arena::with_test_region(|| {
        extern "C" {
            fn strlen(s: *const std::ffi::c_char) -> usize;
        }
        let sig = signature(TypeDesc::Size, vec![TypeDesc::Str], None);
        let cif = prepare_cif(&sig);
        let h = crate::primitives::ctx::TestHeap::new();
        let hello = h.ctx().string("hello");
        let result = crate::primitives::ctx::with_test_ctx(|ctx| unsafe {
            ffi_call(strlen as *const c_void, &[hello], &sig, &cif, ctx)
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_int(), Some(5));
    });
}

// The trap: a C `bool` return defines only the low byte of the return
// register, and a compiler may leave bits set above it. This callee returns
// false that way. Counter-factual: read as a `c_int`, the register is true.
#[test]
fn bool_return_reads_only_the_low_byte() {
    extern "C" fn false_with_upper_bits() -> u32 {
        0x1D0F00
    }
    let sig = signature(TypeDesc::Bool, vec![], None);
    let cif = prepare_cif(&sig);
    let result = crate::primitives::ctx::with_test_ctx(|ctx| unsafe {
        ffi_call(false_with_upper_bits as *const c_void, &[], &sig, &cif, ctx)
    });
    assert_eq!(result.unwrap(), Value::bool(false));
}
