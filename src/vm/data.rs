use super::core::VM;
use crate::value::{cons, error_val, TableKey, Value, SIG_ERROR};

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

/// Car with silent nil: returns nil if not a cons cell.
/// Used by destructuring — missing values become nil, no errors.
pub fn handle_car_or_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on CarOrNil");
    match val.as_cons() {
        Some(cons) => vm.fiber.stack.push(cons.first),
        None => vm.fiber.stack.push(Value::NIL),
    }
}

/// Cdr with silent nil: returns nil if not a cons cell.
/// Used by destructuring — missing values become nil, no errors.
pub fn handle_cdr_or_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on CdrOrNil");
    match val.as_cons() {
        Some(cons) => vm.fiber.stack.push(cons.rest),
        None => vm.fiber.stack.push(Value::NIL),
    }
}

/// Indexed ref with silent nil: returns nil if out of bounds or not an array/tuple.
/// Operand: u16 index (immediate, read from bytecode).
/// Used by destructuring — missing values become nil, no errors.
pub fn handle_array_ref_or_nil(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayRefOrNil");
    let result = if let Some(vec_ref) = val.as_array() {
        vec_ref.borrow().get(index).copied()
    } else if let Some(elems) = val.as_tuple() {
        elems.get(index).copied()
    } else {
        None
    };
    vm.fiber.stack.push(result.unwrap_or(Value::NIL));
}

/// Slice from index with silent nil: returns sub-array from index to end.
/// Works on both arrays and tuples; result is always an array.
/// Operand: u16 index (immediate, read from bytecode).
/// Used by & rest destructuring — collects remaining elements.
pub fn handle_array_slice_from(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArraySliceFrom");
    let result = if let Some(vec_ref) = val.as_array() {
        let borrowed = vec_ref.borrow();
        if index < borrowed.len() {
            Value::array(borrowed[index..].to_vec())
        } else {
            Value::array(vec![])
        }
    } else if let Some(elems) = val.as_tuple() {
        if index < elems.len() {
            Value::array(elems[index..].to_vec())
        } else {
            Value::array(vec![])
        }
    } else {
        Value::array(vec![])
    };
    vm.fiber.stack.push(result);
}

/// Table/struct get with silent nil: returns nil if key missing or wrong type.
/// Operand: u16 constant pool index (keyword key).
/// Used by destructuring — missing keys become nil, no errors.
pub fn handle_table_get_or_nil(vm: &mut VM, bytecode: &[u8], ip: &mut usize, constants: &[Value]) {
    let const_idx = vm.read_u16(bytecode, ip) as usize;
    let key_value = constants[const_idx];
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on TableGetOrNil");

    // Convert the constant to a TableKey for lookup
    let key = if let Some(name) = key_value.as_keyword_name() {
        TableKey::Keyword(name.to_string())
    } else if let Some(s) = key_value.with_string(|s| s.to_string()) {
        TableKey::String(s)
    } else if let Some(i) = key_value.as_int() {
        TableKey::Int(i)
    } else if let Some(id) = key_value.as_symbol() {
        TableKey::Symbol(crate::value::SymbolId(id))
    } else if let Some(b) = key_value.as_bool() {
        TableKey::Bool(b)
    } else if key_value.is_nil() {
        TableKey::Nil
    } else {
        vm.fiber.stack.push(Value::NIL);
        return;
    };

    // Try struct first (immutable, no RefCell borrow)
    if let Some(struct_map) = val.as_struct() {
        if let Some(value) = struct_map.get(&key) {
            vm.fiber.stack.push(*value);
            return;
        }
    }
    // Try table (mutable)
    if let Some(table_ref) = val.as_table() {
        if let Some(value) = table_ref.borrow().get(&key) {
            vm.fiber.stack.push(*value);
            return;
        }
    }
    // Not found or wrong type → nil
    vm.fiber.stack.push(Value::NIL);
}

/// Extend an array with all elements from an indexed source (array or tuple).
/// Stack: \[array, source\] → \[extended_array\]
/// Used by splice: builds the args array incrementally.
pub fn handle_array_extend(vm: &mut VM) {
    let source = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayExtend");
    let array = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayExtend");

    // Get the source elements
    let source_elems: Vec<Value> = if let Some(arr) = source.as_array() {
        arr.borrow().to_vec()
    } else if let Some(tup) = source.as_tuple() {
        tup.to_vec()
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array or tuple, got {}",
                    source.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };

    // Get the target array and extend it
    if let Some(arr) = array.as_array() {
        let mut vec = arr.borrow().to_vec();
        vec.extend(source_elems);
        vm.fiber.stack.push(Value::array(vec));
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array as accumulator, got {}",
                    array.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
    }
}

/// Push a single value onto an array.
/// Stack: \[array, value\] → \[extended_array\]
/// Used by splice: adds non-spliced args to the args array.
pub fn handle_array_push(vm: &mut VM) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayPush");
    let array = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayPush");

    if let Some(arr) = array.as_array() {
        let mut vec = arr.borrow().to_vec();
        vec.push(value);
        vm.fiber.stack.push(Value::array(vec));
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array as accumulator, got {}",
                    array.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
    }
}
