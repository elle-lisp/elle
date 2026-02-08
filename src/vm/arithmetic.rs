use super::core::VM;
use crate::arithmetic;
use crate::value::Value;

pub fn handle_add_int(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    let a = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    vm.stack.push(Value::Int(a + b));
    Ok(())
}

pub fn handle_sub_int(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    let a = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    vm.stack.push(Value::Int(a - b));
    Ok(())
}

pub fn handle_mul_int(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    let a = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    vm.stack.push(Value::Int(a * b));
    Ok(())
}

pub fn handle_div_int(vm: &mut VM) -> Result<(), String> {
    let b_val = vm.stack.pop().ok_or("Stack underflow")?;
    let a_val = vm.stack.pop().ok_or("Stack underflow")?;
    let b = b_val.as_int()?;
    let a = a_val.as_int()?;
    if b == 0 {
        // Create a division-by-zero Condition
        // Exception ID 4 is "division-by-zero" from ExceptionRegistry
        let mut cond = crate::value::Condition::new(4);
        cond.set_field(0, Value::Int(a)); // dividend
        cond.set_field(1, Value::Int(b)); // divisor
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Err("Division by zero".to_string());
    }
    vm.stack.push(Value::Int(a / b));
    Ok(())
}

pub fn handle_add(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    let result = arithmetic::add_values(&a, &b)?;
    vm.stack.push(result);
    Ok(())
}

pub fn handle_sub(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    let result = arithmetic::sub_values(&a, &b)?;
    vm.stack.push(result);
    Ok(())
}

pub fn handle_mul(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    let result = arithmetic::mul_values(&a, &b)?;
    vm.stack.push(result);
    Ok(())
}

pub fn handle_div(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    let result = arithmetic::div_values(&a, &b)?;
    vm.stack.push(result);
    Ok(())
}
