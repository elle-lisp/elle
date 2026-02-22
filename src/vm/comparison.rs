use super::core::VM;
use crate::value::{error_val, Value, SIG_ERROR};

pub fn handle_eq(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    vm.fiber
        .stack
        .push(if a == b { Value::TRUE } else { Value::FALSE });
}

pub fn handle_lt(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Lt");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Lt");
    let result = match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if x < y {
                Value::TRUE
            } else {
                Value::FALSE
            }
        }
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => {
                if x < y {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "<: expected numbers, got {} and {}",
                            a.type_name(),
                            b.type_name()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        },
    };
    vm.fiber.stack.push(result);
}

pub fn handle_gt(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Gt");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Gt");
    let result = match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if x > y {
                Value::TRUE
            } else {
                Value::FALSE
            }
        }
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => {
                if x > y {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            ">: expected numbers, got {} and {}",
                            a.type_name(),
                            b.type_name()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        },
    };
    vm.fiber.stack.push(result);
}

pub fn handle_le(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Le");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Le");
    let result = match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if x <= y {
                Value::TRUE
            } else {
                Value::FALSE
            }
        }
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => {
                if x <= y {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "<=: expected numbers, got {} and {}",
                            a.type_name(),
                            b.type_name()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        },
    };
    vm.fiber.stack.push(result);
}

pub fn handle_ge(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ge");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ge");
    let result = match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if x >= y {
                Value::TRUE
            } else {
                Value::FALSE
            }
        }
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => {
                if x >= y {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            ">=: expected numbers, got {} and {}",
                            a.type_name(),
                            b.type_name()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        },
    };
    vm.fiber.stack.push(result);
}
