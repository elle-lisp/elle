use super::core::VM;
use crate::arithmetic;
use crate::value::Value;

// ---------------------------------------------------------------------------
// Macros to eliminate the binary-op copy-paste
// ---------------------------------------------------------------------------

/// Binary integer op: pop two ints, apply `$op`, push result.
/// Panics on type mismatch — intrinsics are unsafe, Silent means silent.
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
            let a = a_val.as_int().unwrap_or_else(|| {
                panic!(
                    concat!($sym, ": expected integer, got {}"),
                    a_val.type_name()
                )
            });
            let b = b_val.as_int().unwrap_or_else(|| {
                panic!(
                    concat!($sym, ": expected integer, got {}"),
                    b_val.type_name()
                )
            });
            vm.fiber.stack.push(Value::int($op(a, b)));
        }
    };
}

/// Generic binary op via arithmetic module: pop two values, delegate.
/// Panics on type mismatch — intrinsics are unsafe, Silent means silent.
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
                Err(_) => panic!(
                    concat!("%", $instr, ": type error ({} and {})"),
                    a.type_name(),
                    b.type_name()
                ),
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Integer-specialized ops
// ---------------------------------------------------------------------------

int_binop!(handle_add_int, "AddInt", "%add", |a: i64, b: i64| a + b);
int_binop!(handle_sub_int, "SubInt", "%sub", |a: i64, b: i64| a - b);
int_binop!(handle_mul_int, "MulInt", "%mul", |a: i64, b: i64| a * b);

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
    let a = a_val
        .as_int()
        .unwrap_or_else(|| panic!("%div: expected integer, got {}", a_val.type_name()));
    let b = b_val
        .as_int()
        .unwrap_or_else(|| panic!("%div: expected integer, got {}", b_val.type_name()));
    if b == 0 {
        panic!("%div: division by zero");
    }
    vm.fiber.stack.push(Value::int(a / b));
}

// ---------------------------------------------------------------------------
// Generic (mixed int/float) ops
// ---------------------------------------------------------------------------

generic_binop!(handle_add, "add", arithmetic::add_values);
generic_binop!(handle_sub, "sub", arithmetic::sub_values);
generic_binop!(handle_mul, "mul", arithmetic::mul_values);
generic_binop!(handle_rem, "rem", arithmetic::remainder_values);

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

    // Division by zero: panic for pure integer division.
    // Float division follows IEEE 754 (returns Inf/-Inf/NaN).
    if let (Some(_), Some(y)) = (a.as_int(), b.as_int()) {
        if y == 0 {
            panic!("%div: division by zero");
        }
    }

    match arithmetic::div_values(&a, &b) {
        Ok(result) => vm.fiber.stack.push(result),
        Err(_) => panic!("%div: type error ({} and {})", a.type_name(), b.type_name()),
    }
}

// ---------------------------------------------------------------------------
// Bitwise ops
// ---------------------------------------------------------------------------

int_binop!(handle_bit_and, "BitAnd", "%bit-and", |a: i64, b: i64| a & b);
int_binop!(handle_bit_or, "BitOr", "%bit-or", |a: i64, b: i64| a | b);
int_binop!(handle_bit_xor, "BitXor", "%bit-xor", |a: i64, b: i64| a ^ b);

// BitNot is unary
pub(crate) fn handle_bit_not(vm: &mut VM) {
    let a_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on BitNot");
    let a = a_val
        .as_int()
        .unwrap_or_else(|| panic!("%bit-not: expected integer, got {}", a_val.type_name()));
    vm.fiber.stack.push(Value::int(!a));
}

// Shifts need clamping
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
    let a = a_val
        .as_int()
        .unwrap_or_else(|| panic!("%shl: expected integer, got {}", a_val.type_name()));
    let b = b_val
        .as_int()
        .unwrap_or_else(|| panic!("%shl: expected integer, got {}", b_val.type_name()));
    let shift = b.clamp(0, 63) as u32;
    vm.fiber.stack.push(Value::int(a << shift));
}

// ---------------------------------------------------------------------------
// Type conversion ops
// ---------------------------------------------------------------------------

pub(crate) fn handle_int_to_float(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IntToFloat");
    if let Some(n) = val.as_int() {
        vm.fiber.stack.push(Value::float(n as f64));
    } else if val.as_float().is_some() {
        vm.fiber.stack.push(val); // identity
    } else {
        panic!("%float: expected number, got {}", val.type_name());
    }
}

pub(crate) fn handle_float_to_int(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on FloatToInt");
    if let Some(f) = val.as_float() {
        vm.fiber.stack.push(Value::int(f as i64));
    } else if val.as_int().is_some() {
        vm.fiber.stack.push(val); // identity
    } else {
        panic!("%int: expected number, got {}", val.type_name());
    }
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
    let a = a_val
        .as_int()
        .unwrap_or_else(|| panic!("%shr: expected integer, got {}", a_val.type_name()));
    let b = b_val
        .as_int()
        .unwrap_or_else(|| panic!("%shr: expected integer, got {}", b_val.type_name()));
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
    #[should_panic(expected = "%bit-and: expected integer")]
    fn test_handle_bit_and_type_error() {
        let mut vm = make_vm();
        vm.fiber.stack.push(Value::int(12));
        vm.fiber.stack.push(Value::float(10.0));
        handle_bit_and(&mut vm);
    }
}
