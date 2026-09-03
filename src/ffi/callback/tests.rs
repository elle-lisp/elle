//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::ffi::types::{CallingConvention, Signature, TypeDesc};
use crate::value::fiber::SignalBits;
use crate::value::Closure;

fn test_closure(heap: &mut crate::value::fiberheap::FiberHeap, arity: usize) -> Rc<Closure> {
    use crate::value::types::Arity;
    use crate::value::TemplateProto;
    let proto = TemplateProto {
        num_locals: arity,
        num_params: arity,
        ..TemplateProto::new(Vec::new(), Arity::Exact(arity), Vec::new())
    };
    Rc::new(Closure::new(
        crate::value::closure::test_template(heap, proto),
        crate::value::region_slice::RegionSlice::empty(),
        SignalBits::EMPTY,
    ))
}

#[test]
fn test_create_and_free_callback() {
    let mut vm = crate::vm::VM::new();
    let closure = test_closure(vm.heap(), 2);
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::Ptr, TypeDesc::Ptr],
        fixed_args: None,
    };

    // NIL closure-value: these tests create/free the callback without ever
    // invoking it, so no body runs and the register handoff is unexercised.
    let cb = create_callback(closure, Value::NIL, sig, &mut vm as *mut crate::vm::VM).unwrap();
    assert_ne!(cb.code_ptr, 0);
    free_callback(cb);
}

#[test]
fn test_variadic_callback_rejected() {
    let mut vm = crate::vm::VM::new();
    let closure = test_closure(vm.heap(), 2);
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::Ptr, TypeDesc::I32],
        fixed_args: Some(1),
    };

    let result = create_callback(closure, Value::NIL, sig, &mut vm as *mut crate::vm::VM);
    assert!(result.is_err());
}

#[test]
fn test_callback_store() {
    let mut vm = crate::vm::VM::new();
    let closure = test_closure(vm.heap(), 1);
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::Void,
        args: vec![TypeDesc::I32],
        fixed_args: None,
    };

    let mut store = CallbackStore::new();
    let cb = create_callback(closure, Value::NIL, sig, &mut vm as *mut crate::vm::VM).unwrap();
    let ptr = store.insert(cb);
    assert_ne!(ptr, 0);
    assert!(store.remove(ptr));
    assert!(!store.remove(ptr)); // Already removed
}

/// The callback error slot lives on the VM's FFI subsystem (set by the
/// trampoline, drained by `ffi/call`): set then take yields the value once, and
/// the slot is empty before and after.
#[test]
fn test_callback_error_flag() {
    let mut vm = crate::vm::VM::new();
    assert!(vm.ffi_mut().take_callback_error().is_none());
    let region = vm.heap().new_runtime_region();
    let err = crate::value::error_val_in(vm.heap(), "test", "test error", region);
    vm.ffi_mut().set_callback_error(err);
    assert!(vm.ffi_mut().take_callback_error().is_some());
    assert!(vm.ffi_mut().take_callback_error().is_none());
}

#[test]
fn test_zero_result_does_not_crash() {
    // Allocate a buffer and verify zero_result writes zeros
    let mut buf = [0xFFu8; 16];
    unsafe {
        zero_result(&mut *buf.as_mut_ptr().cast::<c_void>(), &TypeDesc::I32);
    }
    // First 4 bytes should be zero (i32 size)
    assert_eq!(&buf[..4], &[0, 0, 0, 0]);
}

#[test]
fn test_zero_result_void() {
    // Void has no size — zero_result should be a no-op
    let mut buf = [0xFFu8; 8];
    unsafe {
        zero_result(&mut *buf.as_mut_ptr().cast::<c_void>(), &TypeDesc::Void);
    }
    // Buffer should be unchanged
    assert_eq!(&buf, &[0xFF; 8]);
}
