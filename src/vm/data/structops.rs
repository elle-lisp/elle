use super::*;

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
        if let Some(value) = sorted_struct_get(struct_map, &key) {
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
            vm.set_error("type-error", "destructuring: invalid key type");
            vm.fiber.stack.push(Value::NIL);
            return;
        }
    };

    // Try immutable struct
    if let Some(struct_map) = val.as_struct() {
        match sorted_struct_get(struct_map, &key) {
            Some(value) => {
                vm.fiber.stack.push(*value);
                return;
            }
            None => {
                vm.set_error(
                    "type-error",
                    format!("destructuring: key {} not found", key_value),
                );
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
                vm.set_error(
                    "type-error",
                    format!("destructuring: key {} not found", key_value),
                );
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        }
    }
    // Not a struct at all
    vm.set_error(
        "type-error",
        format!("destructuring: expected struct, got {}", val.type_name()),
    );
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

    // Collect all keys not in exclude set from struct or @struct. The fresh
    // sub-struct is the destructure-rest binding's value: born in its own fresh
    // region (Rule 3; docs/impl/region-ctx.md) via a `NativeCtx`
    // over a freshly minted region, freed value-based by the binding's consumer,
    // like the native-result discipline.
    if let Some(struct_map) = val.as_struct() {
        let result: Vec<(TableKey, Value)> = struct_map
            .iter()
            .filter(|(k, _)| !exclude.contains(k))
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        let rest = ctx.struct_from_sorted(result);
        vm.fiber.stack.push(rest);
    } else if let Some(table_ref) = val.as_struct_mut() {
        let mut result: Vec<(TableKey, Value)> = table_ref
            .borrow()
            .iter()
            .filter(|(k, _)| !exclude.contains(k))
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        result.sort_by(|(a, _), (b, _)| a.cmp(b));
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        let rest = ctx.struct_from_sorted(result);
        vm.fiber.stack.push(rest);
    } else {
        // Non-struct input → empty struct rest (consistent with StructGetOrNil nil behavior)
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        let rest = ctx.struct_from_sorted(vec![]);
        vm.fiber.stack.push(rest);
    }
}

/// First with silent nil (parameter destructuring): returns nil if not a pair cell.
/// Used for &opt/(required) parameter destructuring where absent values produce nil.
pub(crate) fn handle_car_or_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on FirstOrNil");
    match val.as_pair() {
        Some(pair) => vm.fiber.stack.push(pair.first),
        None => vm.fiber.stack.push(Value::NIL),
    }
}

/// Rest with silent empty-list (parameter destructuring): returns EMPTY_LIST if not a pair cell.
/// Used for &opt/(required) parameter destructuring.
pub(crate) fn handle_cdr_or_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on RestOrNil");
    match val.as_pair() {
        Some(pair) => vm.fiber.stack.push(pair.rest),
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
/// Stack: \[array, source\] → \[array\]
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
    } else if source.as_pair().is_some() || source.is_empty_list() {
        match source.list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                vm.set_error(
                    "type-error",
                    "splice: list is not a proper list (dotted pair)",
                );
                vm.fiber.stack.push(Value::NIL);
                return;
            }
        }
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected array, tuple, or list, got {}",
                source.type_name()
            ),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    };

    if array.is_array_mut() {
        let extended = crate::value::arena::extend_with_incref(
            unsafe { &mut *vm.heap_ptr },
            array,
            &source_elems,
        );
        vm.fiber.stack.push(extended);
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected @array as accumulator, got {}",
                array.type_name()
            ),
        );
        vm.fiber.stack.push(Value::NIL);
    }
}

/// Push a single value onto an array.
/// Stack: \[array, value\] → \[array\]
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

    if array.is_array_mut() {
        let pushed =
            crate::value::arena::push_with_incref(unsafe { &mut *vm.heap_ptr }, array, value);
        vm.fiber.stack.push(pushed);
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected @array as accumulator, got {}",
                array.type_name()
            ),
        );
        vm.fiber.stack.push(Value::NIL);
    }
}
