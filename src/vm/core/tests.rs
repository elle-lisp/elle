use super::*;

#[test]
fn test_signal_bits() {
    use crate::value::{SIG_ERROR, SIG_OK, SIG_YIELD};

    assert_eq!(SIG_OK.raw(), 0);
    assert_eq!(SIG_ERROR.raw(), 1);
    assert_eq!(SIG_YIELD.raw(), 2);

    let mask = SIG_ERROR | SIG_YIELD;
    assert!(mask.intersects(SIG_ERROR));
    assert!(mask.intersects(SIG_YIELD));
    assert!(!mask.intersects(SIG_OK)); // SIG_OK has no bits, so it shares none
}

#[test]
fn test_capture_stack_trace() {
    use std::collections::HashMap;
    let mut vm = VM::new();
    let empty_map = Rc::new(HashMap::new());

    vm.push_call_frame("function_a".to_string(), 10, empty_map.clone());
    vm.push_call_frame("function_b".to_string(), 20, empty_map.clone());
    vm.push_call_frame("function_c".to_string(), 30, empty_map.clone());

    let trace = vm.capture_stack_trace();

    assert_eq!(trace.len(), 3);
    assert_eq!(trace[0].function_name, Some("function_c".to_string()));
    assert_eq!(trace[1].function_name, Some("function_b".to_string()));
    assert_eq!(trace[2].function_name, Some("function_a".to_string()));
}

#[test]
fn test_wrap_error_with_trace() {
    use std::collections::HashMap;
    let mut vm = VM::new();
    let empty_map = Rc::new(HashMap::new());

    vm.push_call_frame("outer".to_string(), 5, empty_map.clone());
    vm.push_call_frame("inner".to_string(), 15, empty_map.clone());

    let error_msg = "Something went wrong".to_string();
    let wrapped = vm.wrap_error(error_msg);

    assert!(wrapped.contains("Something went wrong"));
    assert!(wrapped.contains("inner"));
    assert!(wrapped.contains("outer"));
}

#[test]
fn test_wrap_error_empty_stack() {
    let vm = VM::new();

    let error_msg = "Error with no context".to_string();
    let wrapped = vm.wrap_error(error_msg.clone());

    assert_eq!(wrapped, error_msg);
}

#[test]
fn signal_bits_operand_round_trips_the_user_range() {
    // A `(signal :keyword)` declaration allocates a bit in 32-63, so the
    // operand the emitter writes and the operand the VM reads must agree over
    // the whole 64-bit width (docs/impl/bytecode.md § "Signal-bits operands").
    //
    // The counter-factual: a narrower operand decodes this mask to SIG_OK,
    // which is exactly what a fiber returning normally looks like — the emit
    // then reads as a return, and nothing downstream reports an error.
    use crate::compiler::bytecode::Bytecode;
    use crate::value::fiber::SignalBits;

    let bits = SignalBits::from_bit(32)
        .union(SignalBits::from_bit(63))
        .union(crate::value::SIG_YIELD);
    let mut bc = Bytecode::new();
    bc.emit_signal_bits(bits);
    assert_eq!(bc.instructions.len(), 8, "a signal-bits operand is 8 bytes");

    let vm = VM::new();
    let mut ip = 0;
    assert_eq!(vm.read_signal_bits(&bc.instructions, &mut ip), bits);
    assert_eq!(ip, 8, "reading a signal-bits operand advances 8 bytes");
}
