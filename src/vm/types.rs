use super::core::VM;
use crate::value::Value;

pub fn handle_is_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNil");
    vm.fiber.stack.push(Value::bool(val.is_nil()));
}

pub fn handle_is_pair(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsPair");
    vm.fiber.stack.push(Value::bool(val.is_cons()));
}

pub fn handle_is_number(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNumber");
    vm.fiber.stack.push(Value::bool(val.is_number()));
}

pub fn handle_is_symbol(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSymbol");
    vm.fiber.stack.push(Value::bool(val.is_symbol()));
}

pub fn handle_not(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Not");
    vm.fiber.stack.push(Value::bool(!val.is_truthy()));
}

pub fn handle_is_tuple(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsTuple");
    vm.fiber.stack.push(Value::bool(val.is_tuple()));
}

pub fn handle_is_array(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsArray");
    vm.fiber.stack.push(Value::bool(val.is_array()));
}

pub fn handle_is_struct(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsStruct");
    vm.fiber.stack.push(Value::bool(val.is_struct()));
}

pub fn handle_array_len(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayLen");
    let len = if let Some(a) = val.as_array() {
        a.borrow().len() as i64
    } else if let Some(t) = val.as_tuple() {
        t.len() as i64
    } else {
        0
    };
    vm.fiber.stack.push(Value::int(len));
}

pub fn handle_is_table(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsTable");
    vm.fiber.stack.push(Value::bool(val.is_table()));
}

pub fn handle_is_empty_list(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsEmptyList");
    vm.fiber.stack.push(Value::bool(val.is_empty_list()));
}
