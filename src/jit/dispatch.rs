//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.
//!
//! Data structure/cell helpers are in `data.rs`; yield helpers in `suspend.rs`;
//! function call dispatch in `calls.rs`.
//! Re-exported here so `compiler.rs` / `vtable.rs` can reference them as `dispatch::*`.

use crate::jit::value::JitValue;
use crate::value::fiber::SignalBits;
use crate::value::{sorted_struct_get, Value};

mod structops;
pub use structops::*;

mod region;
pub use region::*;

// Re-export split modules so compiler.rs / vtable.rs can still use dispatch::elle_jit_*
pub use super::calls::*;
pub use super::data::*;
pub use super::suspend::*;

// =============================================================================
// Array and Collection Mutation Helpers
// =============================================================================
/// Push a value onto a mutable @array (splice path). Mutates in place
/// and returns the same @array Value, mirroring `handle_array_push` in
/// `src/vm/data.rs` which goes through `arena::push_with_incref`. Must call
/// `incref_inserted_element` so cross-region references the @array now holds keep
/// the source region alive — without this, the source region can drop
/// to RC=0 and be freed while the @array still references its values,
/// corrupting the C heap on subsequent allocations.
#[no_mangle]
pub extern "C" fn elle_jit_array_push(
    array_tag: u64,
    array_payload: u64,
    val_tag: u64,
    val_payload: u64,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let array_val = Value {
        tag: array_tag,
        payload: array_payload,
    };
    let value_val = Value {
        tag: val_tag,
        payload: val_payload,
    };
    if array_val.is_array_mut() {
        JitValue::from_value(crate::value::arena::push_with_incref(
            unsafe { &mut *vm.heap_ptr },
            array_val,
            value_val,
        ))
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected @array as accumulator, got {}",
                array_val.type_name()
            ),
        );
        JitValue::nil()
    }
}

/// Extend a mutable @array with the elements of another array/list,
/// mutating in place. Returns the same @array Value. Mirrors
/// `handle_array_extend` in `src/vm/data.rs` which goes through
/// `arena::extend_with_incref`; the per-element `incref_inserted_element` keeps any
/// cross-region source values alive.
#[no_mangle]
pub extern "C" fn elle_jit_array_extend(
    array_tag: u64,
    array_payload: u64,
    source_tag: u64,
    source_payload: u64,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let array_val = Value {
        tag: array_tag,
        payload: array_payload,
    };
    let source_val = Value {
        tag: source_tag,
        payload: source_payload,
    };

    let source_elems: Vec<Value> = if let Some(arr) = source_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = source_val.as_array() {
        arr.to_vec()
    } else if source_val.as_pair().is_some() || source_val.is_empty_list() {
        match source_val.list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                vm.set_error(
                    "type-error",
                    "splice: list is not a proper list (dotted pair)",
                );
                return JitValue::nil();
            }
        }
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected array, tuple, or list, got {}",
                source_val.type_name()
            ),
        );
        return JitValue::nil();
    };

    if array_val.is_array_mut() {
        JitValue::from_value(crate::value::arena::extend_with_incref(
            unsafe { &mut *vm.heap_ptr },
            array_val,
            &source_elems,
        ))
    } else {
        vm.set_error(
            "type-error",
            format!(
                "splice: expected array as accumulator, got {}",
                array_val.type_name()
            ),
        );
        JitValue::nil()
    }
}

/// Push a dynamic parameter frame.
/// pairs_ptr: *const Value (16 bytes each), alternating [param, value, param, value, ...]
/// Returns NIL on success or NIL with signal set on error.
#[no_mangle]
pub extern "C" fn elle_jit_push_param_frame(
    pairs_ptr: *const Value,
    count: u64,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let count = count as usize;
    let pairs = unsafe { std::slice::from_raw_parts(pairs_ptr, count * 2) };

    let mut frame = Vec::with_capacity(count);
    for i in 0..count {
        let param = pairs[i * 2];
        let val = pairs[i * 2 + 1];
        if let Some((id, _default)) = param.as_parameter() {
            frame.push((id, val));
        } else {
            vm.set_error(
                "type-error",
                format!("parameterize: {} is not a parameter", param.type_name()),
            );
            return JitValue::nil();
        }
    }
    vm.fiber.param_frames.push(frame);
    JitValue::nil()
}

// =============================================================================
// Struct Access Helpers
// =============================================================================

/// Check that a closure's signal bits are a subset of allowed_bits.
/// Signals error if not. Non-closure values pass silently.
#[no_mangle]
pub extern "C" fn elle_jit_check_signal_bound(
    src_tag: u64,
    src_payload: u64,
    allowed_bits: u64,
    vm: *mut (),
) -> JitValue {
    let val = Value {
        tag: src_tag,
        payload: src_payload,
    };
    let allowed = SignalBits::from_i64(allowed_bits as i64);
    if let Some(closure) = val.as_closure() {
        let signal_bits = closure.signal().bits;
        let excess = signal_bits.subtract(allowed);
        if !excess.is_empty() {
            let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
            let registry = crate::signals::registry::global_registry().lock().unwrap();
            let excess_str = registry.format_signal_bits(excess);
            let allowed_str = registry.format_signal_bits(allowed);
            drop(registry);
            vm_ref.set_error(
                "signal-violation",
                format!(
                    "restrict: closure may emit {} but parameter is restricted to {}",
                    excess_str, allowed_str
                ),
            );
        }
    }
    JitValue::nil()
}

// =============================================================================
// Region (scope) helpers for JIT
// =============================================================================

#[cfg(test)]
mod tests;
