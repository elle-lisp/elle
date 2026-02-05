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
    let b = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    let a = vm.stack.pop().ok_or("Stack underflow")?.as_int()?;
    if b == 0 {
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
