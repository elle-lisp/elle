//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.
//!
//! Data structure/cell helpers are in `data.rs`; yield helpers in `suspend.rs`;
//! function call dispatch in `calls.rs`.
//! Re-exported here so `compiler.rs` / `vtable.rs` can reference them as `dispatch::*`.

use crate::jit::value::JitValue;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, sorted_struct_get, Value};

// Re-export split modules so compiler.rs / vtable.rs can still use dispatch::elle_jit_*
pub use super::calls::*;
pub use super::data::*;
pub use super::suspend::*;

// =============================================================================
// Array and Collection Mutation Helpers
// =============================================================================
/// Push a value onto a mutable @array (splice path). Mutates in place
/// and returns the same @array Value, mirroring `handle_array_push` in
/// `src/vm/data.rs` which goes through `arena::tracked_push`. Must call
/// `track_insert` so cross-region references the @array now holds keep
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
    if array_val.as_array_mut().is_some() {
        JitValue::from_value(crate::value::arena::tracked_push(array_val, value_val))
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected @array as accumulator, got {}",
                    array_val.type_name()
                ),
            ),
        ));
        JitValue::nil()
    }
}

/// Extend a mutable @array with the elements of another array/list,
/// mutating in place. Returns the same @array Value. Mirrors
/// `handle_array_extend` in `src/vm/data.rs` which goes through
/// `arena::tracked_extend`; the per-element `track_insert` keeps any
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
                vm.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        "splice: list is not a proper list (dotted pair)",
                    ),
                ));
                return JitValue::nil();
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
        return JitValue::nil();
    };

    if array_val.as_array_mut().is_some() {
        JitValue::from_value(crate::value::arena::tracked_extend(
            array_val,
            &source_elems,
        ))
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
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("parameterize: {} is not a parameter", param.type_name()),
                ),
            ));
            return JitValue::nil();
        }
    }
    vm.fiber.param_frames.push(frame);
    JitValue::nil()
}

// =============================================================================
// Struct Access Helpers
// =============================================================================

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
            vm_ref.fiber.signal = Some((
                SIG_ERROR,
                error_val("type-error", "destructuring: invalid key type"),
            ));
            return JitValue::nil();
        }
    };
    if let Some(struct_map) = val.as_struct() {
        return match sorted_struct_get(struct_map, &key) {
            Some(v) => JitValue::from_value(*v),
            None => {
                vm_ref.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_val),
                    ),
                ));
                JitValue::nil()
            }
        };
    }
    if let Some(table_ref) = val.as_struct_mut() {
        return match table_ref.borrow().get(&key) {
            Some(v) => JitValue::from_value(*v),
            None => {
                vm_ref.fiber.signal = Some((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("destructuring: key {} not found", key_val),
                    ),
                ));
                JitValue::nil()
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
    _vm: *mut (),
) -> JitValue {
    let val = Value {
        tag: src_tag,
        payload: src_payload,
    };
    let count = count as usize;
    let exclude_vals = unsafe { std::slice::from_raw_parts(exclude_ptr, count) };

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
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        JitValue::from_value(Value::struct_from_sorted(result))
    } else if let Some(table_ref) = val.as_struct_mut() {
        let mut result: Vec<_> = table_ref
            .borrow()
            .iter()
            .filter(|(k, _)| !exclude.contains(k))
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        result.sort_by(|(a, _), (b, _)| a.cmp(b));
        JitValue::from_value(Value::struct_from_sorted(result))
    } else {
        JitValue::from_value(Value::struct_from_sorted(vec![]))
    }
}

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
    JitValue::nil()
}

// =============================================================================
// Region (scope) helpers for JIT
// =============================================================================

/// Legacy scope-mark helpers (no-ops — replaced by `DecrefRegion`).
#[no_mangle]
pub extern "C" fn elle_jit_region_enter() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_exit() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_exit_call() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_rotate() -> JitValue {
    JitValue::nil()
}

/// Increment the reference count of a region (cross-region reference).
#[no_mangle]
pub extern "C" fn elle_jit_incref_region(region_id: u32) {
    let ptr = crate::value::fiberheap::current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).incref_region(region_id as u16) };
    }
}

/// Decrement the reference count of a region (cross-region reference).
#[no_mangle]
pub extern "C" fn elle_jit_decref_region(region_id: u32) {
    let ptr = crate::value::fiberheap::current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).decref_region(region_id as u16) };
    }
}

/// Release a value's region if it matches the expected region id.
///
/// Mirrors the VM dispatch handler for `ReleaseValueRegion`: read
/// `region_of(value)`, gate on `region_id != 0 && region_id == expected`,
/// and decref only on match. Passthrough call results (where the returned
/// value lives in a different region than the one this call allocated) skip
/// the decref.
#[no_mangle]
pub extern "C" fn elle_jit_release_value_region(tag: u64, payload: u64, expected_region_id: u32) {
    let value = Value { tag, payload };
    let region_id = crate::value::arena::region_of(value);
    if region_id != 0 && region_id == expected_region_id as u16 {
        let ptr = crate::value::fiberheap::current_heap_ptr();
        if !ptr.is_null() {
            unsafe { (*ptr).decref_region(region_id) };
        }
    }
}

/// Increment the durable reference count for a heap value.
/// Called by JIT `StoreLocal` to track binding references.
#[no_mangle]
pub extern "C" fn elle_jit_incref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

/// Decrement refcount only (no drop). Called by JIT `StoreLocalRefcounted`
/// to release the old binding's reference. The old value may still be
/// reachable through collections or other bindings — actual freeing is
/// deferred to scope exit.
#[no_mangle]
pub extern "C" fn elle_jit_decref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::value::JitValue;
    use crate::vm::VM;

    /// Regression: elle_jit_array_push must MUTATE the input @array in place
    /// and return the same Value, matching the VM's handle_array_push contract
    /// in `src/vm/data.rs` (`tracked_push`). The earlier implementation cloned
    /// the contents and returned a freshly-allocated @array, which (a) gave
    /// the user-visible wrong semantics for `(push @arr x)` and (b) skipped
    /// the cross-region RC accounting (`track_insert`), letting the source
    /// region of inserted heap values be freed while the @array still held
    /// dangling pointers — eventually corrupting the C heap. Counterfactual
    /// for the corruption in tests/elle/jit-double-import-uaf.lisp.
    #[test]
    fn array_push_mutates_in_place_and_returns_same_value() {
        crate::value::arena::with_test_region(|| {
            use crate::primitives::register_primitives;
            use crate::symbol::SymbolTable;

            let mut symbols = SymbolTable::new();
            let mut vm = VM::new();
            let _signals = register_primitives(&mut vm, &mut symbols);

            let arr = Value::array_mut(vec![]);
            let v = Value::int(42);
            let ret = elle_jit_array_push(
                arr.tag,
                arr.payload,
                v.tag,
                v.payload,
                &mut vm as *mut VM as *mut (),
            );
            let ret_val = Value {
                tag: ret.tag,
                payload: ret.payload,
            };
            // Returned Value must be identical (same heap object) to the input.
            assert_eq!(
                ret_val.tag, arr.tag,
                "elle_jit_array_push must return the same @array (tag mismatch)"
            );
            assert_eq!(
                ret_val.payload, arr.payload,
                "elle_jit_array_push must return the same @array (payload mismatch)"
            );
            // Input @array must reflect the push.
            let inner = arr.as_array_mut().expect("input is @array");
            assert_eq!(
                inner.borrow().len(),
                1,
                "@array length should be 1 after push"
            );
            assert_eq!(
                inner.borrow()[0],
                v,
                "@array element should be the pushed value"
            );
        });
    }

    /// elle_jit_array_push must bump the source region's RC when a heap
    /// Value is inserted into an @array that lives in a different region.
    /// This is what keeps the source region alive across the insertion;
    /// without it the source region can drop to RC=0 and be freed while
    /// the @array still references it, producing the heap corruption that
    /// tests/elle/jit-double-import-uaf.lisp reproduced.
    #[test]
    fn array_push_track_inserts_cross_region_value() {
        use crate::value::arena::{alloc_in_fresh_region, region_rc};
        use crate::value::heap::{HeapObject, Pair};
        crate::value::arena::with_test_region(|| {
            use crate::primitives::register_primitives;
            use crate::symbol::SymbolTable;

            let mut symbols = SymbolTable::new();
            let mut vm = VM::new();
            let _signals = register_primitives(&mut vm, &mut symbols);

            let arr = Value::array_mut(vec![]);
            // Allocate a heap value in a different fresh region.
            let (cross, source_rid) =
                alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
            let rc_before = region_rc(source_rid);
            let _ret = elle_jit_array_push(
                arr.tag,
                arr.payload,
                cross.tag,
                cross.payload,
                &mut vm as *mut VM as *mut (),
            );
            let rc_after = region_rc(source_rid);
            assert_eq!(
                rc_after,
                rc_before + 1,
                "elle_jit_array_push must incref the source region of an inserted cross-region value"
            );
        });
    }

    /// elle_jit_push (the IntrPush intrinsic helper) shares the same
    /// contract: @array mutate-in-place plus track_insert for cross-region
    /// values. Mirrors elle_jit_array_push's tests.
    #[test]
    fn intr_push_track_inserts_cross_region_value() {
        use crate::jit::runtime::elle_jit_push;
        use crate::value::arena::{alloc_in_fresh_region, region_rc};
        use crate::value::heap::{HeapObject, Pair};
        crate::value::arena::with_test_region(|| {
            use crate::primitives::register_primitives;
            use crate::symbol::SymbolTable;

            let mut symbols = SymbolTable::new();
            let mut vm = VM::new();
            let _signals = register_primitives(&mut vm, &mut symbols);

            let arr = Value::array_mut(vec![]);
            let (cross, source_rid) =
                alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
            let rc_before = region_rc(source_rid);
            let ret = elle_jit_push(arr.tag, arr.payload, cross.tag, cross.payload);
            let ret_val = Value {
                tag: ret.tag,
                payload: ret.payload,
            };
            assert_eq!(
                (ret_val.tag, ret_val.payload),
                (arr.tag, arr.payload),
                "elle_jit_push must return the same @array Value"
            );
            let rc_after = region_rc(source_rid);
            assert_eq!(
                rc_after,
                rc_before + 1,
                "elle_jit_push must incref the source region of an inserted cross-region value"
            );
        });
    }

    #[test]
    fn test_has_exception() {
        crate::value::arena::with_test_region(|| {
            use crate::primitives::register_primitives;
            use crate::symbol::SymbolTable;

            let mut symbols = SymbolTable::new();
            let mut vm = VM::new();
            let _signals = register_primitives(&mut vm, &mut symbols);

            // Initially no exception
            let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
            assert_eq!(result, JitValue::bool_val(false));

            // Set an error signal
            vm.fiber.signal = Some((
                crate::value::SIG_ERROR,
                crate::value::error_val("division-by-zero", "test"),
            ));

            // Now should return true
            let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
            assert_eq!(result, JitValue::bool_val(true));

            // Clear signal
            vm.fiber.signal = None;

            // Should return false again
            let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
            assert_eq!(result, JitValue::bool_val(false));
        });
    }
}
