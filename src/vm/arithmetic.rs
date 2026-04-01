use super::core::VM;
use crate::arithmetic;
use crate::value::{error_val, Value, SIG_ERROR};

// ---------------------------------------------------------------------------
// Macros to eliminate the binary-op copy-paste
// ---------------------------------------------------------------------------

/// Binary integer op: pop two ints, apply `$op`, push result.
macro_rules! int_binop {
    ($name:ident, $instr:literal, $sym:literal, $op:expr) => {
        pub(crate) fn $name(vm: &mut VM) {
            let b_val = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $instr));
            let a_val = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $instr));
            let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            concat!($sym, ": expected integers, got {} and {}"),
                            a_val.type_name(),
                            b_val.type_name(),
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            };
            vm.fiber.stack.push(Value::int($op(a, b)));
        }
    };
}

/// Generic binary op via arithmetic module: pop two values, delegate.
macro_rules! generic_binop {
    ($name:ident, $instr:literal, $arith_fn:path) => {
        pub(crate) fn $name(vm: &mut VM) {
            let b = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $instr));
            let a = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $instr));
            match $arith_fn(&a, &b) {
                Ok(result) => vm.fiber.stack.push(result),
                Err(err_val) => {
                    vm.fiber.signal = Some((SIG_ERROR, err_val));
                    vm.fiber.stack.push(Value::NIL);
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Integer-specialized ops
// ---------------------------------------------------------------------------

int_binop!(handle_add_int, "AddInt", "+", |a: i64, b: i64| a + b);
int_binop!(handle_sub_int, "SubInt", "-", |a: i64, b: i64| a - b);
int_binop!(handle_mul_int, "MulInt", "*", |a: i64, b: i64| a * b);

// DivInt needs special div-by-zero handling
pub(crate) fn handle_div_int(vm: &mut VM) {
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

// ---------------------------------------------------------------------------
// Generic (mixed int/float) ops
// ---------------------------------------------------------------------------

generic_binop!(handle_add, "Add", arithmetic::add_values);
generic_binop!(handle_sub, "Sub", arithmetic::sub_values);
generic_binop!(handle_mul, "Mul", arithmetic::mul_values);
generic_binop!(handle_rem, "Rem", arithmetic::remainder_values);

// Div needs the integer div-by-zero pre-check
pub(crate) fn handle_div(vm: &mut VM) {
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

    // Division by zero: error only for pure integer division.
    // Float division follows IEEE 754 (returns Inf/-Inf/NaN).
    if let (Some(_), Some(y)) = (a.as_int(), b.as_int()) {
        if y == 0 {
            vm.fiber.signal = Some((SIG_ERROR, error_val("division-by-zero", "division by zero")));
            vm.fiber.stack.push(Value::NIL);
            return;
        }
    }

    match arithmetic::div_values(&a, &b) {
        Ok(result) => vm.fiber.stack.push(result),
        Err(err_val) => {
            vm.fiber.signal = Some((SIG_ERROR, err_val));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

// ---------------------------------------------------------------------------
// Bitwise ops
// ---------------------------------------------------------------------------

int_binop!(handle_bit_and, "BitAnd", "bit-and", |a: i64, b: i64| a & b);
int_binop!(handle_bit_or, "BitOr", "bit-or", |a: i64, b: i64| a | b);
int_binop!(handle_bit_xor, "BitXor", "bit-xor", |a: i64, b: i64| a ^ b);

// BitNot is unary — no macro
pub(crate) fn handle_bit_not(vm: &mut VM) {
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

// Shifts need clamping — keep explicit
pub(crate) fn handle_shl(vm: &mut VM) {
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
    let shift = b.clamp(0, 63) as u32;
    vm.fiber.stack.push(Value::int(a << shift));
}

pub(crate) fn handle_shr(vm: &mut VM) {
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
