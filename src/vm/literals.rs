use super::core::VM;
use crate::value::Value;

pub(crate) fn handle_nil(vm: &mut VM) {
    vm.fiber.stack.push(Value::NIL);
}

pub(crate) fn handle_empty_list(vm: &mut VM) {
    vm.fiber.stack.push(Value::EMPTY_LIST);
}

pub(crate) fn handle_true(vm: &mut VM) {
    vm.fiber.stack.push(Value::TRUE);
}

pub(crate) fn handle_false(vm: &mut VM) {
    vm.fiber.stack.push(Value::FALSE);
}
