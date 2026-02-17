use super::core::VM;
use crate::value::Value;

pub fn handle_eq(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack
        .push(if a == b { Value::TRUE } else { Value::FALSE });
    Ok(())
}

pub fn handle_lt(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
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
                let cond = crate::value::Condition::type_error(format!(
                    "<: expected numbers, got {} and {}",
                    a.type_name(),
                    b.type_name()
                ));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                vm.stack.push(Value::NIL);
                return Ok(());
            }
        },
    };
    vm.stack.push(result);
    Ok(())
}

pub fn handle_gt(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
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
                let cond = crate::value::Condition::type_error(format!(
                    ">: expected numbers, got {} and {}",
                    a.type_name(),
                    b.type_name()
                ));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                vm.stack.push(Value::NIL);
                return Ok(());
            }
        },
    };
    vm.stack.push(result);
    Ok(())
}

pub fn handle_le(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
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
                let cond = crate::value::Condition::type_error(format!(
                    "<=: expected numbers, got {} and {}",
                    a.type_name(),
                    b.type_name()
                ));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                vm.stack.push(Value::NIL);
                return Ok(());
            }
        },
    };
    vm.stack.push(result);
    Ok(())
}

pub fn handle_ge(vm: &mut VM) -> Result<(), String> {
    let b = vm.stack.pop().ok_or("Stack underflow")?;
    let a = vm.stack.pop().ok_or("Stack underflow")?;
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
                let cond = crate::value::Condition::type_error(format!(
                    ">=: expected numbers, got {} and {}",
                    a.type_name(),
                    b.type_name()
                ));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                vm.stack.push(Value::NIL);
                return Ok(());
            }
        },
    };
    vm.stack.push(result);
    Ok(())
}
