//! Data construction and type-predicate helpers for JIT-compiled code.
//!
//! Constructors (`elle_jit_pair`, `elle_jit_make_array`,
//! `elle_jit_materialize_const`) allocate into the explicit region resolved by
//! `elle_jit_resolve_alloc_region`; the `is_*` predicates are pure tag checks.

use super::region_of_raw;
use crate::jit::value::JitValue;
use crate::value::Value;

/// Allocate a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_pair(
    car_tag: u64,
    car_payload: u64,
    cdr_tag: u64,
    cdr_payload: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    let head = Value {
        tag: car_tag,
        payload: car_payload,
    };
    let tail = Value {
        tag: cdr_tag,
        payload: cdr_payload,
    };
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region/ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    JitValue::from_value(crate::value::build::pair(
        heap,
        head,
        tail,
        region_of_raw(region),
    ))
}

/// Extract car from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_first(pair_tag: u64, pair_payload: u64) -> JitValue {
    let pair = Value {
        tag: pair_tag,
        payload: pair_payload,
    };
    match pair.as_pair() {
        Some(pair) => JitValue::from_value(pair.first),
        None => {
            eprintln!("JIT type error: expected pair");
            JitValue::nil()
        }
    }
}

/// Extract cdr from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_rest(pair_tag: u64, pair_payload: u64) -> JitValue {
    let pair = Value {
        tag: pair_tag,
        payload: pair_payload,
    };
    match pair.as_pair() {
        Some(pair) => JitValue::from_value(pair.rest),
        None => {
            eprintln!("JIT type error: expected pair");
            JitValue::nil()
        }
    }
}

/// Allocate an array from a list of elements
/// elements: *const Value (16 bytes each)
#[no_mangle]
pub extern "C" fn elle_jit_make_array(
    elements: *const Value,
    count: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    let count = count as usize;
    let mut vec = Vec::with_capacity(count);
    for i in 0..count {
        let v = unsafe { *elements.add(i) };
        vec.push(v);
    }
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region/ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    JitValue::from_value(crate::value::build::array_mut(
        heap,
        vec,
        region_of_raw(region),
    ))
}

/// Materialize a FRESH heap literal — a string, or quoted compound data — from a
/// `ConstTemplate` into the explicit `region`. `ptr` is a pointer to a
/// JIT-code-owned `ConstTemplate` that outlives the native code
/// (`FunctionTranslator::templates` → `JitCode`); `materialize` recurses in Rust,
/// building the whole structure into `region`, so the result is independent of the
/// template. Mirrors the interpreter's `literals::handle_materialize_const`.
#[no_mangle]
pub extern "C" fn elle_jit_materialize_const(ptr: i64, region: u32, vm: *mut ()) -> JitValue {
    let template = unsafe { &*(ptr as *const crate::value::ConstTemplate) };
    // A quoted-symbol leaf re-interns into this instance's table via the driving
    // VM — the same table the interpreter's `literals::handle_materialize_const`
    // uses.
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let heap = unsafe { &mut *vm.heap_ptr };
    JitValue::from_value(template.materialize(heap, region_of_raw(region), vm.symbols()))
}

/// Check if value is a pair (pair cell)
#[no_mangle]
pub extern "C" fn elle_jit_is_pair(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_pair())
}

/// Check if value is an immutable array
#[no_mangle]
pub extern "C" fn elle_jit_is_array(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_array())
}

/// Check if value is a mutable @array
#[no_mangle]
pub extern "C" fn elle_jit_is_array_mut(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_array_mut())
}

/// Check if value is an immutable struct
#[no_mangle]
pub extern "C" fn elle_jit_is_struct(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_struct())
}

/// Check if value is a mutable @struct
#[no_mangle]
pub extern "C" fn elle_jit_is_struct_mut(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_struct_mut())
}

/// Check if value is an immutable set
#[no_mangle]
pub extern "C" fn elle_jit_is_set(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_set())
}

/// Check if value is a mutable @set
#[no_mangle]
pub extern "C" fn elle_jit_is_set_mut(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_set_mut())
}
