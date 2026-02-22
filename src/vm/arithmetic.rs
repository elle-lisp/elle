use super::core::VM;
use crate::arithmetic;
use crate::value::{error_val, Value, SIG_ERROR};

pub fn handle_add_int(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on AddInt");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on AddInt");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "+: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a + b));
}

pub fn handle_sub_int(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on SubInt");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on SubInt");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "-: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a - b));
}

pub fn handle_mul_int(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on MulInt");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on MulInt");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "*: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a * b));
}

pub fn handle_div_int(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on DivInt");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on DivInt");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "/: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    if b == 0 {
        vm.fiber.signal = Some((SIG_ERROR, error_val("division-by-zero", "division by zero")));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    vm.fiber.stack.push(Value::int(a / b));
}

pub fn handle_add(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Add");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Add");
    match arithmetic::add_values(&a, &b) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_sub(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Sub");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Sub");
    match arithmetic::sub_values(&a, &b) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_mul(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Mul");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Mul");
    match arithmetic::mul_values(&a, &b) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_div(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Div");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Div");

    // Check for division by zero and set exception instead of returning error
    let is_zero = match (a.as_int(), b.as_int()) {
        (Some(_), Some(y)) => y == 0,
        _ => match (a.as_float(), b.as_float()) {
            (Some(_), Some(y)) => y == 0.0,
            _ => match (a.as_int(), b.as_float()) {
                (Some(_), Some(y)) => y == 0.0,
                _ => match (a.as_float(), b.as_int()) {
                    (Some(_), Some(y)) => y == 0,
                    _ => false,
                },
            },
        },
    };

    if is_zero {
        vm.fiber.signal = Some((SIG_ERROR, error_val("division-by-zero", "division by zero")));
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    match arithmetic::div_values(&a, &b) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_rem(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Rem");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Rem");
    match arithmetic::remainder_values(&a, &b) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("type-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_bit_and(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitAnd");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitAnd");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "bit-and: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a & b));
}

pub fn handle_bit_or(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitOr");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitOr");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "bit-or: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a | b));
}

pub fn handle_bit_xor(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitXor");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitXor");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "bit-xor: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(a ^ b));
}

pub fn handle_bit_not(vm: &mut VM) {
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitNot");
    let Some(a) = a_val.as_int() else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bit-not: expected integer, got {}", a_val.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(Value::int(!a));
}

pub fn handle_shl(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Shl");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Shl");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "shl: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    // Clamp shift amount to valid range to avoid panic
    let shift = b.clamp(0, 63) as u32;
    vm.fiber.stack.push(Value::int(a << shift));
}

pub fn handle_shr(vm: &mut VM) {
    let b_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Shr");
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Shr");
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "shr: expected integers, got {} and {}",
                    a_val.type_name(),
                    b_val.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    // Clamp shift amount to valid range to avoid panic
    let shift = b.clamp(0, 63) as u32;
    vm.fiber.stack.push(Value::int(a >> shift));
}

#[cfg(test)]
mod tests {
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
    fn test_handle_bit_and_type_error() {
        let mut vm = make_vm();
        vm.fiber.stack.push(Value::int(12));
        vm.fiber.stack.push(Value::float(10.0));
        handle_bit_and(&mut vm);
        // Should set signal and push NIL
        assert!(vm.fiber.signal.is_some());
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
    }
}
