// audited: 2026-10-22
//! Runtime dispatch helpers for JIT-compiled code: array mutation, parameter
//! frames, the signal bound, and re-exports.
//!
//! docs/impl/jit.md
//!
//! Data structure and cell helpers are in `data.rs`, yield helpers in
//! `suspend.rs`, struct access in `structops.rs`, region accounting in
//! `region.rs` and function call dispatch in `calls.rs`. All are re-exported
//! here so `compiler.rs` and `vtable.rs` reference them as `dispatch::*`.

use crate::jit::value::JitValue;
use crate::value::fiber::SignalBits;
use crate::value::{sorted_struct_get, Value};

mod structops;
pub use structops::*;

mod region;
pub use region::*;

// Re-export the split modules, so compiler.rs and vtable.rs reach them as dispatch::elle_jit_*
pub use super::calls::*;
pub use super::data::*;
pub use super::suspend::*;

// =============================================================================
// Array and Collection Mutation Helpers
// =============================================================================
/// Push a value onto a mutable @array (splice path). Mutates in place and
/// returns the same @array Value, mirroring `handle_array_push` in
/// src/vm/data/structops.rs. Both go through `arena::push_with_incref`, whose
/// `incref_inserted_element` keeps the source region of a cross-region value
/// alive. Without it, the source region can drop to RC=0 and free while the
/// @array still references its values.
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
/// `handle_array_extend` in src/vm/data/structops.rs, which goes through
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

    let source_elems: Vec<Value> = if let Some(elems) = source_val.array_to_vec() {
        elems
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
///
/// A non-parameter raises and pushes an empty frame, exactly as the
/// interpreter's `PushParamFrame` does: a restart continues into the body,
/// whose scope-end `PopParamFrame` pops that empty frame and not the one
/// below it.
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
            vm.fiber.param_frames.push(Vec::new());
            return JitValue::nil();
        }
    }
    vm.fiber.param_frames.push(frame);
    JitValue::nil()
}

// =============================================================================
// Signal Bound Check
// =============================================================================

/// Check that the value's signal bits are a subset of allowed_bits, as the
/// interpreter's `CheckSignalBound` does. `signals::bound::violation` judges a
/// closure by its effective signal and a native by its declared one, and lets
/// any other value pass. Signals error on a violation.
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
    if let Some(message) = crate::signals::bound::violation(val, allowed) {
        let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
        vm_ref.set_error("signal-violation", message);
    }
    JitValue::nil()
}

#[cfg(test)]
mod tests;
