//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::ffi::types::{CallingConvention, Signature};

#[test]
fn test_prepare_cif() {
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::I32],
        fixed_args: None,
    };
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_prepare_cif_no_args() {
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::Void,
        args: vec![],
        fixed_args: None,
    };
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_prepare_variadic_cif() {
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::Ptr, TypeDesc::Size, TypeDesc::Ptr, TypeDesc::I32],
        fixed_args: Some(3),
    };
    let _cif = prepare_cif(&sig);
}

#[test]
fn test_arity_check() {
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::I32],
        fixed_args: None,
    };
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
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::Int,
        args: vec![TypeDesc::Int],
        fixed_args: None,
    };
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
        let sig = Signature {
            convention: CallingConvention::Default,
            ret: TypeDesc::Size,
            args: vec![TypeDesc::Str],
            fixed_args: None,
        };
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
