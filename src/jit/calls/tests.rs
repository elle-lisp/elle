use super::*;
use crate::value::fiber::{SIG_DEBUG, SIG_IO, SIG_OK, SIG_YIELD};
use crate::vm::VM;

fn make_vm() -> VM {
    VM::new()
}

#[test]
fn test_has_exception() {
    crate::value::arena::with_test_region(|| {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // Initially no exception
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));

        // Set an error signal
        let err = vm.escaping_error("division-by-zero", "test");
        vm.fiber.signal = Some((crate::value::SIG_ERROR, err));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));
    });
}

// -- jit_handle_primitive_signal: composed signal coverage --

#[test]
fn sig_ok_returns_value() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_OK, Value::int(42));
    assert_eq!(result, JitValue::from_value(Value::int(42)));
    assert!(vm.fiber.signal.is_none());
}

#[test]
fn bare_sig_yield_stores_signal_returns_yield_sentinel() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_YIELD, Value::int(1));
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, val) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_YIELD);
    assert_eq!(val.as_int(), Some(1));
}

#[test]
fn composed_sig_yield_io_stores_signal_returns_yield_sentinel() {
    let mut vm = make_vm();
    let bits = SIG_YIELD | SIG_IO;
    let result = jit_handle_primitive_signal(&mut vm, bits, Value::int(99));
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, val) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, bits);
    assert_eq!(val.as_int(), Some(99));
}

#[test]
fn sig_halt_stores_signal_returns_nil() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_HALT, Value::int(0));
    assert_eq!(result, JitValue::nil());
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_HALT);
}

#[test]
fn sig_debug_treated_as_suspension() {
    let mut vm = make_vm();
    vm.fiber.signal = Some((SIG_DEBUG, Value::NIL));
    let result = jit_handle_primitive_signal(&mut vm, SIG_DEBUG, Value::NIL);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_DEBUG);
}

#[test]
fn user_defined_signal_treated_as_suspension() {
    let user_bit = SignalBits::from_bit(32);
    let mut vm = make_vm();
    vm.fiber.signal = Some((user_bit, Value::NIL));
    let result = jit_handle_primitive_signal(&mut vm, user_bit, Value::NIL);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, user_bit);
}

// -- Restored tests (wrongly deleted in 94cd2050) --

#[test]
fn bare_sig_error_stores_signal_returns_nil() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let err = h.ctx().string("boom");
        let result = jit_handle_primitive_signal(&mut vm, SIG_ERROR, err);
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_ERROR);
    });
}

#[test]
fn composed_sig_error_io_stores_signal_returns_nil() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let bits = SIG_ERROR | SIG_IO;
        let result = jit_handle_primitive_signal(&mut vm, bits, h.ctx().string("io-error"));
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        assert!(sig.intersects(SIG_IO));
    });
}

#[test]
fn sig_error_terminal_stored_as_error_not_panic() {
    crate::value::arena::with_test_region(|| {
        use crate::value::fiber::SIG_TERMINAL;
        let bits = SIG_ERROR | SIG_TERMINAL;
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, bits, h.ctx().string("terminal"));
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        assert!(sig.intersects(SIG_TERMINAL));
    });
}
