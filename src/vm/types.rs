use super::core::VM;
use crate::value::Value;

pub fn handle_is_nil(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(val.is_nil()));
    Ok(())
}

pub fn handle_is_pair(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(val.is_cons()));
    Ok(())
}

pub fn handle_is_number(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(val.is_number()));
    Ok(())
}

pub fn handle_is_symbol(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(val.is_symbol()));
    Ok(())
}

pub fn handle_not(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(!val.is_truthy()));
    Ok(())
}

pub fn handle_is_empty_list(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(Value::bool(val.is_empty_list()));
    Ok(())
}
