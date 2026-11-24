// audited: 2026-11-25
//! Destructuring and safe-accessor helpers for JIT-compiled code, and the syntax view `match` reads through.
//!
//! docs/impl/jit.md
//!
//! The `*_destructure` helpers signal a `type-error` on shape mismatch (pattern
//! binding must fail loudly), while `*_or_nil` variants and `elle_jit_array_len`
//! silently degrade — the split mirrors the interpreter's two accessor moods.
//! `elle_jit_match_fail` signals `:match-error` when no arm covered the value.

use crate::jit::value::JitValue;
use crate::value::{Mutability, Value};

/// No match arm covered the scrutinee: signals :match-error carrying it.
#[no_mangle]
pub extern "C" fn elle_jit_match_fail(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let err = vm.escaping_match_fail(val);
    vm.fiber.signal = Some((crate::value::SIG_ERROR, err));
    JitValue::nil()
}

/// What `match` reads a value as: a syntax object's view, and any other value
/// unchanged (docs/macros/syntax.md). Never signals.
#[no_mangle]
pub extern "C" fn elle_jit_syntax_view(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    JitValue::from_value(crate::vm::types::syntax_view(vm, val))
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
    let found = match val.array_mut_read() {
        Some(read) => Some((read.get(idx).copied(), read.len())),
        None => val
            .as_array()
            .map(|elems| (elems.get(idx).copied(), elems.len())),
    };
    match found {
        Some((Some(v), _)) => JitValue::from_value(v),
        Some((None, len)) => {
            vm_ref.set_error(
                "type-error",
                format!("destructuring: array index {idx} out of bounds (length {len})"),
            );
            JitValue::nil()
        }
        None => {
            vm_ref.set_error(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            );
            JitValue::nil()
        }
    }
}

/// ArraySliceFrom: the array of an array's elements from `index` on. Signals
/// a type error on any other value, an @array included.
#[no_mangle]
pub extern "C" fn elle_jit_array_slice_from(
    tag: u64,
    payload: u64,
    index: u64,
    vm: *mut (),
) -> JitValue {
    slice_from(Value { tag, payload }, index, vm, Mutability::Immutable)
}

/// ArrayMutSliceFrom: as [`elle_jit_array_slice_from`], for an @array alone.
#[no_mangle]
pub extern "C" fn elle_jit_array_mut_slice_from(
    tag: u64,
    payload: u64,
    index: u64,
    vm: *mut (),
) -> JitValue {
    slice_from(Value { tag, payload }, index, vm, Mutability::Mutable)
}

/// The slice of `kind` both helpers build, through the interpreter's
/// `array_tail` so every tier refuses the other kind alike.
fn slice_from(val: Value, index: u64, vm: *mut (), kind: Mutability) -> JitValue {
    // Like the interpreter's handler (src/vm/data.rs), the slice has no region
    // slot in the LIR; the result is built through a `NativeCtx` over a freshly
    // minted region (from the VM's own heap).
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let Some(elems) = crate::vm::data::array_tail(val, index as usize, kind) else {
        vm_ref.set_error("type-error", crate::vm::data::slice_refusal(val, kind));
        return JitValue::nil();
    };
    let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm_ref.heap_ptr });
    JitValue::from_value(match kind {
        Mutability::Mutable => ctx.array_mut(elems),
        Mutability::Immutable => ctx.array(elems),
    })
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
    let len = if let Some(arr) = val.array_mut_read() {
        arr.len() as i64
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
    let result = if let Some(arr) = val.array_mut_read() {
        arr.get(idx).copied()
    } else if let Some(arr) = val.as_array() {
        arr.get(idx).copied()
    } else {
        None
    };
    JitValue::from_value(result.unwrap_or(Value::NIL))
}
