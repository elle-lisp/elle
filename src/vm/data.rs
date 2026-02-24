use super::core::VM;
use crate::value::{cons, error_val, Value, SIG_ERROR};

pub fn handle_cons(vm: &mut VM) {
    let rest = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Cons");
    let first = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Cons");
    vm.fiber.stack.push(cons(first, rest));
}

pub fn handle_car(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Car");

    // car of nil is an error - enforces proper list invariant
    if val.is_nil() {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", "car: cannot take car of nil"),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // car of empty list is an error
    if val.is_empty_list() {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", "car: cannot take car of empty list"),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // Handle cons cells
    if let Some(cons) = val.as_cons() {
        vm.fiber.stack.push(cons.first);
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("car: expected cons cell, got {}", val.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
    }
}

pub fn handle_cdr(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Cdr");

    // cdr of nil is an error - enforces proper list invariant
    if val.is_nil() {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", "cdr: cannot take cdr of nil"),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // cdr of empty list is an error
    if val.is_empty_list() {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", "cdr: cannot take cdr of empty list"),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // Handle cons cells
    if let Some(cons) = val.as_cons() {
        vm.fiber.stack.push(cons.rest);
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("cdr: expected cons cell, got {}", val.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
    }
}

pub fn handle_make_array(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let size = vm.read_u8(bytecode, ip) as usize;
    let mut vec = Vec::with_capacity(size);
    for _ in 0..size {
        vec.push(
            vm.fiber
                .stack
                .pop()
                .expect("VM bug: Stack underflow on MakeArray"),
        );
    }
    vec.reverse();
    vm.fiber.stack.push(Value::array(vec));
}

pub fn handle_array_ref(vm: &mut VM) {
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayRef");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayRef");
    let Some(idx_val) = idx.as_int() else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("array-ref: expected integer index, got {}", idx.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    let Some(vec_ref) = vec.as_array() else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("array-ref: expected array, got {}", vec.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    let vec_borrow = vec_ref.borrow();
    match vec_borrow.get(idx_val as usize) {
        Some(val) => {
            vm.fiber.stack.push(*val);
        }
        None => {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "error",
                    format!(
                        "array-ref: index {} out of bounds (length {})",
                        idx_val,
                        vec_borrow.len()
                    ),
                ),
            ));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub fn handle_array_set(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArraySet");
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArraySet");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArraySet");
    let Some(_idx_val) = idx.as_int() else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "array-set!: expected integer index, got {}",
                    idx.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    if vec.as_array().is_none() {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("array-set!: expected array, got {}", vec.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    // Note: Arrays are immutable in this implementation
    vm.fiber.stack.push(val);
}
