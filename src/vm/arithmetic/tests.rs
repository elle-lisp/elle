//! Unit tests (`super` is the parent impl module).

use super::*;

fn make_vm() -> VM {
    VM::new()
}

#[test]
fn test_handle_rem() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(17));
    vm.fiber.stack.push(Value::int(5));
    handle_rem(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(2)));
}

#[test]
fn test_handle_rem_negative() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(-17));
    vm.fiber.stack.push(Value::int(5));
    handle_rem(&mut vm);
    // Remainder has same sign as dividend
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(-2)));
}

#[test]
fn test_handle_bit_and() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(0b1100)); // 12
    vm.fiber.stack.push(Value::int(0b1010)); // 10
    handle_bit_and(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(0b1000))); // 8
}

#[test]
fn test_handle_bit_or() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(0b1100)); // 12
    vm.fiber.stack.push(Value::int(0b1010)); // 10
    handle_bit_or(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(0b1110))); // 14
}

#[test]
fn test_handle_bit_xor() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(0b1100)); // 12
    vm.fiber.stack.push(Value::int(0b1010)); // 10
    handle_bit_xor(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(0b0110))); // 6
}

#[test]
fn test_handle_bit_not() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(0));
    handle_bit_not(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(-1))); // !0 = -1 in two's complement
}

#[test]
fn test_handle_shl() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(1));
    vm.fiber.stack.push(Value::int(4));
    handle_shl(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(16))); // 1 << 4 = 16
}

#[test]
fn test_handle_shr() {
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(16));
    vm.fiber.stack.push(Value::int(2));
    handle_shr(&mut vm);
    assert_eq!(vm.fiber.stack.pop(), Some(Value::int(4))); // 16 >> 2 = 4
}

#[test]
fn test_handle_bit_and_type_mismatch_produces_garbage() {
    // With unchecked intrinsics, wrong types produce garbage, not panics
    let mut vm = make_vm();
    vm.fiber.stack.push(Value::int(12));
    vm.fiber.stack.push(Value::float(10.0));
    handle_bit_and(&mut vm);
    // Should produce some value (garbage), not panic
    assert!(vm.fiber.stack.pop().is_some());
}
