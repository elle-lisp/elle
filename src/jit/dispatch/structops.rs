use super::*;

/// Struct/table get with silent nil: returns value for key, NIL if missing or wrong type.
#[no_mangle]
pub extern "C" fn elle_jit_struct_get_or_nil(
    src_tag: u64,
    src_payload: u64,
    key_tag: u64,
    key_payload: u64,
    _vm: *mut (),
) -> JitValue {
    let val = Value {
        tag: src_tag,
        payload: src_payload,
    };
    let key_val = Value {
        tag: key_tag,
        payload: key_payload,
    };
    let key = match crate::value::heap::TableKey::from_value(&key_val) {
        Some(k) => k,
        None => return JitValue::nil(),
    };
    if let Some(struct_map) = val.as_struct() {
        if let Some(v) = sorted_struct_get(struct_map, &key) {
            return JitValue::from_value(*v);
        }
    }
    if let Some(table_ref) = val.as_struct_mut() {
        if let Some(v) = table_ref.borrow().get(&key) {
            return JitValue::from_value(*v);
        }
    }
    JitValue::nil()
}

/// Struct/table get for destructuring: returns value for key, signals error if missing.
#[no_mangle]
pub extern "C" fn elle_jit_struct_get_destructure(
    src_tag: u64,
    src_payload: u64,
    key_tag: u64,
    key_payload: u64,
    vm: *mut (),
) -> JitValue {
    let val = Value {
        tag: src_tag,
        payload: src_payload,
    };
    let key_val = Value {
        tag: key_tag,
        payload: key_payload,
    };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let key = match crate::value::heap::TableKey::from_value(&key_val) {
        Some(k) => k,
        None => {
            vm_ref.set_error("type-error", "destructuring: invalid key type");
            return JitValue::nil();
        }
    };
    if let Some(struct_map) = val.as_struct() {
        return match sorted_struct_get(struct_map, &key) {
            Some(v) => JitValue::from_value(*v),
            None => {
                vm_ref.set_error(
                    "type-error",
                    format!("destructuring: key {} not found", key_val),
                );
                JitValue::nil()
            }
        };
    }
    if let Some(table_ref) = val.as_struct_mut() {
        return match table_ref.borrow().get(&key) {
            Some(v) => JitValue::from_value(*v),
            None => {
                vm_ref.set_error(
                    "type-error",
                    format!("destructuring: key {} not found", key_val),
                );
                JitValue::nil()
            }
        };
    }
    vm_ref.set_error(
        "type-error",
        format!("destructuring: expected struct, got {}", val.type_name()),
    );
    JitValue::nil()
}

/// Struct rest: collect all keys from src NOT in exclude_keys into a new immutable struct.
/// exclude_ptr: *const Value (16 bytes each), pointing to `count` keyword Values.
#[no_mangle]
pub extern "C" fn elle_jit_struct_rest(
    src_tag: u64,
    src_payload: u64,
    exclude_ptr: *const Value,
    count: u64,
    vm: *mut (),
) -> JitValue {
    let val = Value {
        tag: src_tag,
        payload: src_payload,
    };
    let count = count as usize;
    let exclude_vals = unsafe { std::slice::from_raw_parts(exclude_ptr, count) };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };

    let mut exclude = std::collections::BTreeSet::new();
    for &key_val in exclude_vals {
        if let Some(k) = crate::value::heap::TableKey::from_value(&key_val) {
            exclude.insert(k);
        }
    }

    if let Some(struct_map) = val.as_struct() {
        let result: Vec<_> = struct_map
            .iter()
            .filter(|(k, _)| !exclude.contains(k))
            .map(|(k, v)| (*k, *v))
            .collect();
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
        JitValue::from_value(ctx.struct_from_sorted(result))
    } else if let Some(table_ref) = val.as_struct_mut() {
        let mut result: Vec<_> = table_ref
            .borrow()
            .iter()
            .filter(|(k, _)| !exclude.contains(k))
            .map(|(k, v)| (*k, *v))
            .collect();
        result.sort_by_key(|(a, _)| *a);
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
        JitValue::from_value(ctx.struct_from_sorted(result))
    } else {
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
        JitValue::from_value(ctx.struct_from_sorted(vec![]))
    }
}
