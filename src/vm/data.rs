use super::core::VM;
use crate::value::{cons, error_val, TableKey, Value, SIG_ERROR};

pub(crate) fn handle_cons(vm: &mut VM) {
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

pub(crate) fn handle_car(vm: &mut VM) {
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

pub(crate) fn handle_cdr(vm: &mut VM) {
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

pub(crate) fn handle_make_array(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let size = vm.read_u8(bytecode, ip) as usize;
    let mut vec = Vec::with_capacity(size);
    for _ in 0..size {
        vec.push(
            vm.fiber
                .stack
                .pop()
                .expect("VM bug: Stack underflow on MakeArrayMut"),
        );
    }
    vec.reverse();
    vm.fiber.stack.push(Value::array_mut(vec));
}

pub(crate) fn handle_array_ref(vm: &mut VM) {
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRef");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRef");
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
    let Some(vec_ref) = vec.as_array_mut() else {
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

pub(crate) fn handle_array_set(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
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
    if vec.as_array_mut().is_none() {
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

/// Car for destructuring: signals error if not a cons cell.
pub(crate) fn handle_car_destructure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on CarDestructure");
    match val.as_cons() {
        Some(cons) => vm.fiber.stack.push(cons.first),
        None => {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

/// Cdr for destructuring: signals error if not a cons cell.
pub(crate) fn handle_cdr_destructure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on CdrDestructure");
    match val.as_cons() {
        Some(cons) => vm.fiber.stack.push(cons.rest),
        None => {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
            vm.fiber.stack.push(Value::EMPTY_LIST);
        }
    }
}

/// Array ref for destructuring: signals error if not an array or out of bounds.
/// Operand: u16 index (immediate, read from bytecode).
pub(crate) fn handle_array_ref_destructure(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRefDestructure");
    if let Some(vec_ref) = val.as_array_mut() {
        let borrowed = vec_ref.borrow();
        match borrowed.get(index).copied() {
            Some(v) => vm.fiber.stack.push(v),
            None => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "destructuring: array index {} out of bounds (length {})",
                            index,
                            borrowed.len()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
            }
        }
    } else if let Some(elems) = val.as_array() {
        match elems.get(index).copied() {
            Some(v) => vm.fiber.stack.push(v),
            None => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "destructuring: array index {} out of bounds (length {})",
                            index,
                            elems.len()
                        ),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
            }
        }
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
    }
}

/// Array slice from index for destructuring: returns sub-array from index to end.
/// Works on both arrays and @arrays; result type matches input type.
/// Empty slice (index >= length) is valid. Signals error on wrong type.
/// Operand: u16 index (immediate, read from bytecode).
/// Used by & rest destructuring.
pub(crate) fn handle_array_slice_from(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSliceFrom");
    let result = if let Some(vec_ref) = val.as_array_mut() {
        let borrowed = vec_ref.borrow();
        if index < borrowed.len() {
            Value::array_mut(borrowed[index..].to_vec())
        } else {
            Value::array_mut(vec![])
        }
    } else if let Some(elems) = val.as_array() {
        if index < elems.len() {
            Value::array(elems[index..].to_vec())
        } else {
            Value::array(vec![])
        }
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(result);
}

/// Table/struct get with silent nil: returns nil if key missing or wrong type.
/// Used by pattern matching (match) — absent keys are valid there.
/// Operand: u16 constant pool index (keyword key).
pub(crate) fn handle_struct_get_or_nil(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) {
    let const_idx = vm.read_u16(bytecode, ip) as usize;
    let key_value = constants[const_idx];
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StructGetOrNil");

    // Convert the constant to a TableKey for lookup
    let key = match TableKey::from_value(&key_value) {
        Some(k) => k,
        None => {
            vm.fiber.stack.push(Value::NIL);
            return;
        }
    };

    // Try struct first (immutable, no RefCell borrow)
    if let Some(struct_map) = val.as_struct() {
        if let Some(value) = struct_map.get(&key) {
            vm.fiber.stack.push(*value);
            return;
        }
    }
    // Try table (mutable)
    if let Some(table_ref) = val.as_struct_mut() {
        if let Some(value) = table_ref.borrow().get(&key) {
            vm.fiber.stack.push(*value);
            return;
        }
    }
    // Not found or wrong type → nil
    vm.fiber.stack.push(Value::NIL);
}

/// Table/struct get for destructuring: signals error if key missing or wrong type.
/// Operand: u16 constant pool index (keyword key).
pub(crate) fn handle_struct_get_destructure(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) {
    let const_idx = vm.read_u16(bytecode, ip) as usize;
    let key_value = constants[const_idx];
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StructGetDestructure");

    let key = match TableKey::from_value(&key_value) {
        Some(k) => k,
        None => {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val("type-error", "destructuring: invalid key type"),
            ));
            vm.fiber.stack.push(Value::NIL);
            return;
        }
    };

    // Try immutable struct
    if let Some(struct_map) = val.as_struct() {
        match struct_map.get(&key) {
            Some(value) => {
                vm.fiber.stack.push(*value);
                return;
            }
            None => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_value),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        }
    }
    // Try mutable @struct
    if let Some(table_ref) = val.as_struct_mut() {
        match table_ref.borrow().get(&key) {
            Some(value) => {
                vm.fiber.stack.push(*value);
                return;
            }
            None => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_value),
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        }
    }
    // Not a struct at all
    vm.fiber.signal = Some((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("destructuring: expected struct, got {}", val.type_name()),
        ),
    ));
    vm.fiber.stack.push(Value::NIL);
}

/// Struct rest for destructuring: collect all keys NOT in exclude_keys into a new immutable struct.
/// Operands: u16 count, then count x u16 const_idx.
/// Pops source value from stack, pushes result struct.
pub(crate) fn handle_struct_rest(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) {
    let count = vm.read_u16(bytecode, ip) as usize;
    let mut exclude: std::collections::BTreeSet<TableKey> = std::collections::BTreeSet::new();
    for _ in 0..count {
        let const_idx = vm.read_u16(bytecode, ip) as usize;
        let key_value = constants[const_idx];
        if let Some(k) = TableKey::from_value(&key_value) {
            exclude.insert(k);
        }
    }

    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StructRest");

    // Collect all keys not in exclude set from struct or @struct
    let mut result: std::collections::BTreeMap<TableKey, Value> = std::collections::BTreeMap::new();

    if let Some(struct_map) = val.as_struct() {
        for (k, v) in struct_map.iter() {
            if !exclude.contains(k) {
                result.insert(k.clone(), *v);
            }
        }
    } else if let Some(table_ref) = val.as_struct_mut() {
        for (k, v) in table_ref.borrow().iter() {
            if !exclude.contains(k) {
                result.insert(k.clone(), *v);
            }
        }
    }
    // Non-struct input → empty struct rest (consistent with StructGetOrNil nil behavior)

    vm.fiber.stack.push(Value::struct_from(result));
}

/// Car with silent nil (parameter destructuring): returns nil if not a cons cell.
/// Used for &opt/(required) parameter destructuring where absent values produce nil.
pub(crate) fn handle_car_or_nil(vm: &mut VM) {
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

/// Cdr with silent empty-list (parameter destructuring): returns EMPTY_LIST if not a cons cell.
/// Used for &opt/(required) parameter destructuring.
pub(crate) fn handle_cdr_or_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on CdrOrNil");
    match val.as_cons() {
        Some(cons) => vm.fiber.stack.push(cons.rest),
        None => vm.fiber.stack.push(Value::EMPTY_LIST),
    }
}

/// Array ref with silent nil (parameter destructuring): returns nil if out of bounds or not array.
/// Operand: u16 index (immediate, read from bytecode).
pub(crate) fn handle_array_ref_or_nil(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRefOrNil");
    let result = if let Some(vec_ref) = val.as_array_mut() {
        vec_ref.borrow().get(index).copied()
    } else if let Some(elems) = val.as_array() {
        elems.get(index).copied()
    } else {
        None
    };
    vm.fiber.stack.push(result.unwrap_or(Value::NIL));
}

/// Extend an @array with all elements from an indexed source (array or @array).
/// Stack: \[array, source\] → \[extended_array\]
/// Used by splice: builds the args array incrementally.
pub(crate) fn handle_array_extend(vm: &mut VM) {
    let source = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutExtend");
    let array = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutExtend");

    // Get the source elements
    let source_elems: Vec<Value> = if let Some(arr) = source.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(tup) = source.as_array() {
        tup.to_vec()
    } else if source.as_cons().is_some() || source.is_empty_list() {
        match source.list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        "splice: list is not a proper list (dotted pair)",
                    ),
                ));
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        }
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array, tuple, or list, got {}",
                    source.type_name()
                ),
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };

    // Get the target array and extend it
    if let Some(arr) = array.as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.extend(source_elems);
        vm.fiber.stack.push(Value::array_mut(vec));
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
pub(crate) fn handle_array_push(vm: &mut VM) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutPush");
    let array = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutPush");

    if let Some(arr) = array.as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.push(value);
        vm.fiber.stack.push(Value::array_mut(vec));
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
