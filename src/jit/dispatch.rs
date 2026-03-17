//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.
//!
//! Data structure/cell helpers are in `data.rs`; yield helpers in `suspend.rs`;
//! function call dispatch in `calls.rs`.
//! Re-exported here so `compiler.rs` / `vtable.rs` can reference them as `dispatch::*`.

use crate::value::fiber::SIG_ERROR;
use crate::value::repr::TAG_NIL;
use crate::value::{error_val, Value};

// Re-export split modules so compiler.rs / vtable.rs can still use dispatch::elle_jit_*
pub use super::calls::*;
pub use super::data::*;
pub use super::suspend::*;

// =============================================================================
// Array and Collection Mutation Helpers
// =============================================================================
/// Push a value onto a mutable @array. Returns new @array or TAG_NIL on error.
#[no_mangle]
pub extern "C" fn elle_jit_array_push(array: u64, value: u64, vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let array_val = unsafe { Value::from_bits(array) };
    let value_val = unsafe { Value::from_bits(value) };
    if let Some(arr) = array_val.as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.push(value_val);
        Value::array_mut(vec).to_bits()
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array as accumulator, got {}",
                    array_val.type_name()
                ),
            ),
        ));
        TAG_NIL
    }
}

/// Extend a mutable @array with elements from another array/list. Returns new @array or TAG_NIL on error.
#[no_mangle]
pub extern "C" fn elle_jit_array_extend(array: u64, source: u64, vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let array_val = unsafe { Value::from_bits(array) };
    let source_val = unsafe { Value::from_bits(source) };

    let source_elems: Vec<Value> = if let Some(arr) = source_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = source_val.as_array() {
        arr.to_vec()
    } else if source_val.as_cons().is_some() || source_val.is_empty_list() {
        match source_val.list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        "splice: list is not a proper list (dotted pair)",
                    ),
                ));
                return TAG_NIL;
            }
        }
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array, tuple, or list, got {}",
                    source_val.type_name()
                ),
            ),
        ));
        return TAG_NIL;
    };

    if let Some(arr) = array_val.as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.extend(source_elems);
        Value::array_mut(vec).to_bits()
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array as accumulator, got {}",
                    array_val.type_name()
                ),
            ),
        ));
        TAG_NIL
    }
}

/// Push a dynamic parameter frame. pairs_ptr points to alternating [param, value, param, value, ...]
/// Returns TAG_NIL on success, TAG_NIL with signal set on error.
#[no_mangle]
pub extern "C" fn elle_jit_push_param_frame(pairs_ptr: u64, count: u64, vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let count = count as usize;
    let pairs = unsafe { std::slice::from_raw_parts(pairs_ptr as *const u64, count * 2) };

    let mut frame = Vec::with_capacity(count);
    for i in 0..count {
        let param_bits = pairs[i * 2];
        let val_bits = pairs[i * 2 + 1];
        let param = unsafe { Value::from_bits(param_bits) };
        let val = unsafe { Value::from_bits(val_bits) };
        if let Some((id, _default)) = param.as_parameter() {
            frame.push((id, val));
        } else {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("parameterize: {} is not a parameter", param.type_name()),
                ),
            ));
            return TAG_NIL;
        }
    }
    vm.fiber.param_frames.push(frame);
    TAG_NIL
}

// =============================================================================
// Struct Access Helpers
// =============================================================================

/// Struct/table get with silent nil: returns value for key, NIL if missing or wrong type.
#[no_mangle]
pub extern "C" fn elle_jit_struct_get_or_nil(src: u64, key: u64, _vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(src) };
    let key_val = unsafe { Value::from_bits(key) };
    let key = match crate::value::heap::TableKey::from_value(&key_val) {
        Some(k) => k,
        None => return TAG_NIL,
    };
    if let Some(struct_map) = val.as_struct() {
        if let Some(v) = struct_map.get(&key) {
            return v.to_bits();
        }
    }
    if let Some(table_ref) = val.as_struct_mut() {
        if let Some(v) = table_ref.borrow().get(&key) {
            return v.to_bits();
        }
    }
    TAG_NIL
}

/// Struct/table get for destructuring: returns value for key, signals error if missing or wrong type.
#[no_mangle]
pub extern "C" fn elle_jit_struct_get_destructure(src: u64, key: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(src) };
    let key_val = unsafe { Value::from_bits(key) };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let key = match crate::value::heap::TableKey::from_value(&key_val) {
        Some(k) => k,
        None => {
            vm_ref.fiber.signal = Some((
                SIG_ERROR,
                error_val("type-error", "destructuring: invalid key type"),
            ));
            return TAG_NIL;
        }
    };
    if let Some(struct_map) = val.as_struct() {
        return match struct_map.get(&key) {
            Some(v) => v.to_bits(),
            None => {
                vm_ref.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_val),
                    ),
                ));
                TAG_NIL
            }
        };
    }
    if let Some(table_ref) = val.as_struct_mut() {
        return match table_ref.borrow().get(&key) {
            Some(v) => v.to_bits(),
            None => {
                vm_ref.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_val),
                    ),
                ));
                TAG_NIL
            }
        };
    }
    vm_ref.fiber.signal = Some((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("destructuring: expected struct, got {}", val.type_name()),
        ),
    ));
    TAG_NIL
}

/// Struct rest: collect all keys from src NOT in exclude_keys into a new immutable struct.
/// exclude_ptr points to an array of count u64 NaN-boxed keyword Values.
#[no_mangle]
pub extern "C" fn elle_jit_struct_rest(
    src: u64,
    exclude_ptr: u64,
    count: u64,
    _vm: *mut (),
) -> u64 {
    let val = unsafe { Value::from_bits(src) };
    let count = count as usize;
    let exclude_bits = unsafe { std::slice::from_raw_parts(exclude_ptr as *const u64, count) };

    let mut exclude = std::collections::BTreeSet::new();
    for &bits in exclude_bits {
        let key_val = unsafe { Value::from_bits(bits) };
        if let Some(k) = crate::value::heap::TableKey::from_value(&key_val) {
            exclude.insert(k);
        }
    }

    let mut result = std::collections::BTreeMap::new();
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
    Value::struct_from(result).to_bits()
}

/// Check that a closure's signal bits are a subset of allowed_bits.
/// Signals error if not. Non-closure values pass silently.
#[no_mangle]
pub extern "C" fn elle_jit_check_signal_bound(src: u64, allowed_bits: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(src) };
    let allowed = allowed_bits as u32;
    if let Some(closure) = val.as_closure() {
        let signal_bits = closure.signal().bits.0;
        let excess = signal_bits & !allowed;
        if excess != 0 {
            let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
            let registry = crate::signals::registry::global_registry().lock().unwrap();
            let excess_str = registry.format_signal_bits(crate::value::fiber::SignalBits(excess));
            let allowed_str = registry.format_signal_bits(crate::value::fiber::SignalBits(allowed));
            drop(registry);
            vm_ref.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "signal-violation",
                    format!(
                        "restrict: closure may emit {} but parameter is restricted to {}",
                        excess_str, allowed_str
                    ),
                ),
            ));
        }
    }
    TAG_NIL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_exception() {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;
        use crate::vm::VM;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // Initially no exception
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));

        // Set an error signal
        vm.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val("division-by-zero", "test"),
        ));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));
    }
}
