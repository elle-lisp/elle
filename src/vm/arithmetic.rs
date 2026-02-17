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
