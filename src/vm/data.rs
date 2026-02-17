use super::core::VM;
use crate::value::{cons, Value};

pub fn handle_cons(vm: &mut VM) -> Result<(), String> {
    let rest = vm.stack.pop().ok_or("Stack underflow")?;
    let first = vm.stack.pop().ok_or("Stack underflow")?;
    vm.stack.push(cons(first, rest));
    Ok(())
}

pub fn handle_car(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;

    // car of nil is an error - enforces proper list invariant
    if val.is_nil() {
        let cond = crate::value::Condition::type_error("car: cannot take car of nil");
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    }

    // car of empty list is an error
    if val.is_empty_list() {
        let cond = crate::value::Condition::type_error("car: cannot take car of empty list");
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    }

    // Handle cons cells
    if let Some(cons) = val.as_cons() {
        vm.stack.push(cons.first);
        Ok(())
    } else {
        let cond = crate::value::Condition::type_error(format!(
            "car: expected cons cell, got {}",
            val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        Ok(())
    }
}

pub fn handle_cdr(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;

    // cdr of nil is an error - enforces proper list invariant
    if val.is_nil() {
        let cond = crate::value::Condition::type_error("cdr: cannot take cdr of nil");
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    }

    // cdr of empty list is an error
    if val.is_empty_list() {
        let cond = crate::value::Condition::type_error("cdr: cannot take cdr of empty list");
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    }

    // Handle cons cells
    if let Some(cons) = val.as_cons() {
        vm.stack.push(cons.rest);
        Ok(())
    } else {
        let cond = crate::value::Condition::type_error(format!(
            "cdr: expected cons cell, got {}",
            val.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        Ok(())
    }
}

pub fn handle_make_vector(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let size = vm.read_u8(bytecode, ip) as usize;
    let mut vec = Vec::with_capacity(size);
    for _ in 0..size {
        vec.push(vm.stack.pop().ok_or("Stack underflow")?);
    }
    vec.reverse();
    vm.stack.push(Value::vector(vec));
    Ok(())
}

pub fn handle_vector_ref(vm: &mut VM) -> Result<(), String> {
    let idx = vm.stack.pop().ok_or("Stack underflow")?;
    let vec = vm.stack.pop().ok_or("Stack underflow")?;
    let Some(idx_val) = idx.as_int() else {
        let cond = crate::value::Condition::type_error(format!(
            "vector-ref: expected integer index, got {}",
            idx.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    let Some(vec_ref) = vec.as_vector() else {
        let cond = crate::value::Condition::type_error(format!(
            "vector-ref: expected vector, got {}",
            vec.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    let vec_borrow = vec_ref.borrow();
    match vec_borrow.get(idx_val as usize) {
        Some(val) => {
            vm.stack.push(*val);
            Ok(())
        }
        None => {
            let cond = crate::value::Condition::error(format!(
                "vector-ref: index {} out of bounds (length {})",
                idx_val,
                vec_borrow.len()
            ));
            vm.current_exception = Some(std::rc::Rc::new(cond));
            vm.stack.push(Value::NIL);
            Ok(())
        }
    }
}

pub fn handle_vector_set(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    let idx = vm.stack.pop().ok_or("Stack underflow")?;
    let vec = vm.stack.pop().ok_or("Stack underflow")?;
    let Some(_idx_val) = idx.as_int() else {
        let cond = crate::value::Condition::type_error(format!(
            "vector-set!: expected integer index, got {}",
            idx.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    };
    if vec.as_vector().is_none() {
        let cond = crate::value::Condition::type_error(format!(
            "vector-set!: expected vector, got {}",
            vec.type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        vm.stack.push(Value::NIL);
        return Ok(());
    }
    // Note: Vectors are immutable in this implementation
    vm.stack.push(val);
    Ok(())
}
