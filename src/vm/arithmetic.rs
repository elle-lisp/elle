use super::core::VM;
use crate::arithmetic;
use crate::value::Value;

pub fn handle_add_int(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "+: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a + b));
    Ok(())
}

pub fn handle_sub_int(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "-: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a - b));
    Ok(())
}

pub fn handle_mul_int(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "*: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a * b));
    Ok(())
}

pub fn handle_div_int(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "/: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    if b == 0 {
        // Create a division-by-zero Condition
        let cond = crate::value::Condition::division_by_zero("division by zero")
            .with_field(0, Value::int(a)) // dividend
            .with_field(1, Value::int(b)); // divisor
        vm.current_exception = Some(std::rc::Rc::new(cond));
        // Push a marker value (nil) to keep stack consistent
        // The exception interrupt mechanism will handle the exception
        vm.stack.push(Value::NIL);
        return Ok(());
    }
    vm.stack.push(Value::int(a / b));
    Ok(())
}

pub fn handle_add(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    match arithmetic::add_values(&a, &b) {
        Ok(result) => {
            vm.stack.push(result);
            Ok(())
        }
        Err(msg) => {
            let cond = crate::value::Condition::type_error(msg);
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_sub(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    match arithmetic::sub_values(&a, &b) {
        Ok(result) => {
            vm.stack.push(result);
            Ok(())
        }
        Err(msg) => {
            let cond = crate::value::Condition::type_error(msg);
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_mul(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    match arithmetic::mul_values(&a, &b) {
        Ok(result) => {
            vm.stack.push(result);
            Ok(())
        }
        Err(msg) => {
            let cond = crate::value::Condition::type_error(msg);
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_div(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;

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
        // Create a division-by-zero Condition
        let cond = crate::value::Condition::division_by_zero("division by zero")
            .with_field(0, a) // dividend
            .with_field(1, b); // divisor
        vm.current_exception = Some(std::rc::Rc::new(cond));
        // Push a marker value (nil) to keep stack consistent
        vm.stack.push(Value::NIL);
        return Ok(());
    }

    match arithmetic::div_values(&a, &b) {
        Ok(result) => {
            vm.stack.push(result);
            Ok(())
        }
        Err(msg) => {
            let cond = crate::value::Condition::type_error(msg);
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_rem(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    match arithmetic::remainder_values(&a, &b) {
        Ok(result) => {
            vm.stack.push(result);
            Ok(())
        }
        Err(msg) => {
            let cond = crate::value::Condition::type_error(msg);
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_bit_and(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "bit-and: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a & b));
    Ok(())
}

pub fn handle_bit_or(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "bit-or: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a | b));
    Ok(())
}

pub fn handle_bit_xor(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "bit-xor: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(a ^ b));
    Ok(())
}

pub fn handle_bit_not(vm: &mut VM) -> Result<(), String> {
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let Some(a) = a_val.as_int() else {
        let cond = crate::value::Condition::type_error(format!(
            "bit-not: expected integer, got {}",
            a_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    vm.stack.push(Value::int(!a));
    Ok(())
}

pub fn handle_shl(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "shl: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    // Clamp shift amount to valid range to avoid panic
    let shift = b.clamp(0, 63) as u32;
    vm.stack.push(Value::int(a << shift));
    Ok(())
}

pub fn handle_shr(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let (Some(a), Some(b)) = (a_val.as_int(), b_val.as_int()) else {
        let cond = crate::value::Condition::type_error(format!(
            "shr: expected integers, got {} and {}",
            a_val.type_name(),
            b_val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    // Clamp shift amount to valid range to avoid panic
    let shift = b.clamp(0, 63) as u32;
    vm.stack.push(Value::int(a >> shift));
    Ok(())
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
        vm.stack.push(Value::int(17));
        vm.stack.push(Value::int(5));
        handle_rem(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(2)));
    }

    #[test]
    fn test_handle_rem_negative() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(-17));
        vm.stack.push(Value::int(5));
        handle_rem(&mut vm).unwrap();
        // Remainder has same sign as dividend
        assert_eq!(vm.stack.pop(), Some(Value::int(-2)));
    }

    #[test]
    fn test_handle_bit_and() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(0b1100)); // 12
        vm.stack.push(Value::int(0b1010)); // 10
        handle_bit_and(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(0b1000))); // 8
    }

    #[test]
    fn test_handle_bit_or() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(0b1100)); // 12
        vm.stack.push(Value::int(0b1010)); // 10
        handle_bit_or(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(0b1110))); // 14
    }

    #[test]
    fn test_handle_bit_xor() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(0b1100)); // 12
        vm.stack.push(Value::int(0b1010)); // 10
        handle_bit_xor(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(0b0110))); // 6
    }

    #[test]
    fn test_handle_bit_not() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(0));
        handle_bit_not(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(-1))); // !0 = -1 in two's complement
    }

    #[test]
    fn test_handle_shl() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(1));
        vm.stack.push(Value::int(4));
        handle_shl(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(16))); // 1 << 4 = 16
    }

    #[test]
    fn test_handle_shr() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(16));
        vm.stack.push(Value::int(2));
        handle_shr(&mut vm).unwrap();
        assert_eq!(vm.stack.pop(), Some(Value::int(4))); // 16 >> 2 = 4
    }

    #[test]
    fn test_handle_bit_and_type_error() {
        let mut vm = make_vm();
        vm.stack.push(Value::int(12));
        vm.stack.push(Value::float(10.0));
        handle_bit_and(&mut vm).unwrap();
        // Should set exception and push NIL
        assert!(vm.current_exception.is_some());
        assert_eq!(vm.stack.pop(), Some(Value::NIL));
    }
}
