use super::core::VM;
use crate::value::Value;

pub fn handle_nil(vm: &mut VM) {
    vm.stack.push(Value::NIL);
}

pub fn handle_empty_list(vm: &mut VM) {
    vm.stack.push(Value::EMPTY_LIST);
}

pub fn handle_true(vm: &mut VM) {
    vm.stack.push(Value::TRUE);
}

pub fn handle_false(vm: &mut VM) {
    vm.stack.push(Value::FALSE);
}
