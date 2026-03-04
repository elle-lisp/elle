use super::core::VM;
use crate::value::{error_val, Value, SIG_ERROR};

pub fn handle_eq(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    // Fast path: bitwise identical
    if a == b {
        vm.fiber.stack.push(Value::TRUE);
        return;
    }
    // Numeric coercion: int 1 == float 1.0
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            vm.fiber
                .stack
                .push(if x == y { Value::TRUE } else { Value::FALSE });
            return;
        }
    }
    vm.fiber.stack.push(Value::FALSE);
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
                if let Some(ord) = a.compare_str(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_lt()));
                    return;
                }
                if let Some(ord) = a.compare_keyword(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_lt()));
                    return;
                }
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "<: expected number, string, or keyword, got {} and {}",
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
                if let Some(ord) = a.compare_str(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_gt()));
                    return;
                }
                if let Some(ord) = a.compare_keyword(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_gt()));
                    return;
                }
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            ">: expected number, string, or keyword, got {} and {}",
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
                if let Some(ord) = a.compare_str(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_le()));
                    return;
                }
                if let Some(ord) = a.compare_keyword(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_le()));
                    return;
                }
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "<=: expected number, string, or keyword, got {} and {}",
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
                if let Some(ord) = a.compare_str(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_ge()));
                    return;
                }
                if let Some(ord) = a.compare_keyword(&b) {
                    vm.fiber.stack.push(Value::bool(ord.is_ge()));
                    return;
                }
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            ">=: expected number, string, or keyword, got {} and {}",
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
