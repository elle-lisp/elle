//! Destructuring and safe-accessor helpers for JIT-compiled code.
//!
//! The `*_destructure` helpers signal a `type-error` on shape mismatch (pattern
//! binding must fail loudly), while `*_or_nil` variants and `elle_jit_array_len`
//! silently degrade — the split mirrors the interpreter's two accessor moods.
//! `elle_jit_match_fail` raises `:match-error` when no arm covered the value.

use crate::jit::value::JitValue;
use crate::value::Value;

/// No match arm covered the scrutinee: signals :match-error carrying it.
#[no_mangle]
pub extern "C" fn elle_jit_match_fail(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let err = vm.escaping_match_fail(val);
    vm.fiber.signal = Some((crate::value::SIG_ERROR, err));
    JitValue::nil()
}

/// First for destructuring: returns car if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_first_destructure(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    match val.as_pair() {
        Some(pair) => JitValue::from_value(pair.first),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.set_error(
                "type-error",
                format!("destructuring: expected list, got {}", val.type_name()),
            );
            JitValue::nil()
        }
    }
}

/// Rest for destructuring: returns cdr if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_rest_destructure(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    match val.as_pair() {
        Some(pair) => JitValue::from_value(pair.rest),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.set_error(
                "type-error",
                format!("destructuring: expected list, got {}", val.type_name()),
            );
            JitValue::nil()
        }
    }
}

/// Array ref for destructuring: signals error if out of bounds or not an array.
#[no_mangle]
pub extern "C" fn elle_jit_array_ref_destructure(
    tag: u64,
    payload: u64,
    index: u64,
    vm: *mut (),
) -> JitValue {
    let val = Value { tag, payload };
    let idx = index as usize;
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        match borrowed.get(idx).copied() {
            Some(v) => JitValue::from_value(v),
            None => {
                vm_ref.set_error(
                    "type-error",
                    format!(
                        "destructuring: array index {} out of bounds (length {})",
                        idx,
                        borrowed.len()
                    ),
                );
                JitValue::nil()
            }
        }
    } else if let Some(elems) = val.as_array() {
        match elems.get(idx).copied() {
            Some(v) => JitValue::from_value(v),
            None => {
                vm_ref.set_error(
                    "type-error",
                    format!(
                        "destructuring: array index {} out of bounds (length {})",
                        idx,
                        elems.len()
                    ),
                );
                JitValue::nil()
            }
        }
    } else {
        vm_ref.set_error(
            "type-error",
            format!("destructuring: expected array, got {}", val.type_name()),
        );
        JitValue::nil()
    }
}

/// Array slice from index: returns sub-array from index to end, preserving mutability.
/// Signals error if not an array.
#[no_mangle]
pub extern "C" fn elle_jit_array_slice_from(
    tag: u64,
    payload: u64,
    index: u64,
    vm: *mut (),
) -> JitValue {
    // NOTE: like the interpreter's `handle_array_slice_from` (src/vm/data.rs),
    // this destructure-slice has no region slot in the LIR; the result is built
    // through a `NativeCtx` over a freshly minted region (from the VM's own heap).
    let val = Value { tag, payload };
    let idx = index as usize;
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        let slice = if idx < borrowed.len() {
            borrowed[idx..].to_vec()
        } else {
            vec![]
        };
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
        JitValue::from_value(ctx.array_mut(slice))
    } else if let Some(elems) = val.as_array() {
        let slice = if idx < elems.len() {
            elems[idx..].to_vec()
        } else {
            vec![]
        };
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
        JitValue::from_value(ctx.array(slice))
    } else {
        vm_ref.set_error(
            "type-error",
            format!("destructuring: expected array, got {}", val.type_name()),
        );
        JitValue::nil()
    }
}

/// First with silent nil: returns car if cons, NIL otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_first_or_nil(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    match val.as_pair() {
        Some(pair) => JitValue::from_value(pair.first),
        None => JitValue::nil(),
    }
}

/// Rest with silent empty-list: returns cdr if cons, EMPTY_LIST otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_rest_or_nil(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    match val.as_pair() {
        Some(pair) => JitValue::from_value(pair.rest),
        None => JitValue::empty_list(),
    }
}

/// Array length: returns length as int for array or @array, 0 otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_array_len(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    let len = if let Some(arr) = val.as_array_mut() {
        arr.borrow().len() as i64
    } else if let Some(arr) = val.as_array() {
        arr.len() as i64
    } else {
        0
    };
    JitValue::from_value(Value::int(len))
}

/// Array ref with silent nil: returns element at index, NIL if out of bounds or not array.
#[no_mangle]
pub extern "C" fn elle_jit_array_ref_or_nil(tag: u64, payload: u64, index: u64) -> JitValue {
    let val = Value { tag, payload };
    let idx = index as usize;
    let result = if let Some(arr) = val.as_array_mut() {
        arr.borrow().get(idx).copied()
    } else if let Some(arr) = val.as_array() {
        arr.get(idx).copied()
    } else {
        None
    };
    JitValue::from_value(result.unwrap_or(Value::NIL))
}
