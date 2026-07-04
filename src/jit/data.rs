//! Data structure and cell helpers for JIT-compiled code

use crate::hir::region::RuntimeRegion;
use crate::jit::value::JitValue;
use crate::value::Value;

/// Decode a raw JIT alloc-region id (resolved by `elle_jit_resolve_alloc_region`)
/// into a `RuntimeRegion`. Always a mortal region (≥ 2) by the emitter invariant.
#[inline]
fn region_of_raw(region: u32) -> RuntimeRegion {
    RuntimeRegion::new(region).expect("JIT alloc region id is a live mortal region")
}

// =============================================================================
// Data Construction
// =============================================================================

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
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region-ctx.md).
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
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region-ctx.md).
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

// =============================================================================
// Box Operations
// =============================================================================

/// Create a LocalCell wrapping a value
#[no_mangle]
pub extern "C" fn elle_jit_make_capture(
    tag: u64,
    payload: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    let val = Value { tag, payload };
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region-ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    JitValue::from_value(crate::value::build::capture_cell(
        heap,
        val,
        region_of_raw(region),
    ))
}

/// Create a CaptureCell in its OWN fresh per-execution region — the JIT-prologue
/// analog of the interpreter's `populate_env`/`push_param` capture-cell path
/// (`env_value_region` mints a fresh `new_runtime_region`, `alloc_in_region`
/// builds the cell there; src/vm/env.rs). The prologue must use THIS, not
/// `elle_jit_make_capture` (which builds the cell in the region slot it is
/// handed): on a JIT->JIT call the callee inherits the caller's region, so a cell
/// allocated there commingles with it (docs/impl/region-rules.md Rule 6) and its
/// value-based `DecrefCellRegion` decrefs the caller's region — a leak (Rule 8)
/// and a latent use-after-free. `alloc_in_region` → `alloc_obj` scans the cell
/// and increfs the wrapped value's region (the cross-region capture edge),
/// exactly as the interpreter does.
#[no_mangle]
pub extern "C" fn elle_jit_make_capture_owned(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    use crate::value::heap::HeapObject;
    use std::cell::RefCell;
    use std::rc::Rc;
    let val = Value { tag, payload };
    // The driving instance's heap, reached through the threaded vm pointer
    // (docs/impl/region-ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let region = heap.new_runtime_region();
    let obj = HeapObject::CaptureCell {
        cell: Rc::new(RefCell::new(val)),
        traits: Value::NIL,
    };
    JitValue::from_value(heap.alloc_in_region(obj, region))
}

/// Build a rest-arg list from `args[start..nargs]`, the JIT-prologue analog of
/// the interpreter's `VM::args_to_list` (src/vm/env.rs): EACH cons is born in
/// its OWN fresh per-execution region, built tail→head so each new cons pins the
/// prior head via its `rest` (whose region `alloc_obj` increfs), and the minting
/// reference on that prior head is then dropped — leaving the chain owned solely
/// head→…→tail so freeing the head cascades the whole list. The prologue must
/// use THIS, not an inline `elle_jit_pair` cons-loop (which allocs into the
/// caller's current region — the same Rule-6/Rule-8 defect as the capture cells).
#[no_mangle]
pub extern "C" fn elle_jit_collect_rest_list(
    args_ptr: *const Value,
    start: u32,
    nargs: u32,
    vm: *mut (),
) -> JitValue {
    use crate::value::heap::{HeapObject, HeapTag, Pair};
    let mut list = Value::EMPTY_LIST;
    if nargs <= start {
        // No rest args — the empty list (no allocation).
        return JitValue::from_value(list);
    }
    // This instance's own heap, via the threaded vm pointer.
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let mut i = nargs;
    while i > start {
        i -= 1;
        let arg = unsafe { *args_ptr.add(i as usize) };
        let cons_region = heap.new_runtime_region();
        let traits = crate::primitives::traitregistry::default_traits_for(heap, HeapTag::Pair);
        let obj = HeapObject::Pair(Pair {
            first: arg,
            rest: list,
            traits,
        });
        // `alloc_in_region` → `alloc_obj` increfs every cross-region ref in the
        // object: the prior head (this cons's `rest`) and any heap `first`. Both
        // are balanced by the free-time cascade.
        let new_cons = heap.alloc_in_region(obj, cons_region);
        // Drop the minting ref on the prior head now that `new_cons` pins it via
        // `rest` — leaving it owned solely by the new cons's edge. EMPTY_LIST has
        // no region (the first cons's `rest`), so `region_of` no-ops there.
        if let Some(prior) = crate::value::arena::region_of(heap, list) {
            if prior != cons_region {
                heap.decref_region(prior);
            }
        }
        list = new_cons;
    }
    JitValue::from_value(list)
}

/// Load value from a CaptureCell
#[no_mangle]
pub extern "C" fn elle_jit_load_capture_cell(cell_tag: u64, cell_payload: u64) -> JitValue {
    let cell = Value {
        tag: cell_tag,
        payload: cell_payload,
    };
    if let Some(cell_ref) = cell.as_capture_cell() {
        JitValue::from_value(*cell_ref.borrow())
    } else {
        eprintln!("JIT type error: expected capture cell");
        JitValue::nil()
    }
}

/// Load from env slot, auto-unwrapping LocalCell if present.
/// This matches the interpreter's LoadUpvalue semantics:
/// - LocalCell (compiler-created mutable capture): unwrap and return inner value
/// - Everything else (plain value, user Cell, etc.): return as-is
#[no_mangle]
pub extern "C" fn elle_jit_load_capture(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    if val.is_capture_cell() {
        if let Some(cell_ref) = val.as_capture_cell() {
            JitValue::from_value(*cell_ref.borrow())
        } else {
            JitValue { tag, payload } // shouldn't happen, but safe fallback
        }
    } else {
        JitValue { tag, payload }
    }
}

/// Store value into a CaptureCell
#[no_mangle]
pub extern "C" fn elle_jit_store_capture_cell(
    cell_tag: u64,
    cell_payload: u64,
    val_tag: u64,
    val_payload: u64,
    vm: *mut (),
) -> JitValue {
    let cell = Value {
        tag: cell_tag,
        payload: cell_payload,
    };
    let val = Value {
        tag: val_tag,
        payload: val_payload,
    };
    if cell.is_capture_cell() {
        // The funnel tracks cross-region refs relative to the cell's region
        // — the JIT twin of the interpreter's UpdateCapture (Rule 5,
        // capture store); the raw store here was an uncounted store. The heap is
        // the driving VM's own, via the threaded vm pointer.
        let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
        crate::value::arena::capture_store_with_rebind(heap, cell, val);
    } else {
        eprintln!("JIT type error: expected capture cell");
    }
    JitValue::nil()
}

/// Store to a capture slot, handling cells automatically.
/// If the slot contains a LocalCell, stores into the cell.
/// Otherwise, stores directly to the slot.
/// env_ptr: *mut Value (16 bytes each)
#[no_mangle]
pub extern "C" fn elle_jit_store_capture(
    env_ptr: *mut Value,
    index: u64,
    val_tag: u64,
    val_payload: u64,
    vm: *mut (),
) -> JitValue {
    let idx = index as usize;
    let slot = unsafe { *env_ptr.add(idx) };
    let new_val = Value {
        tag: val_tag,
        payload: val_payload,
    };

    if slot.is_capture_cell() {
        // The funnel tracks cross-region refs relative to the cell's region
        // — the JIT twin of the interpreter's StoreUpvalue (Rule 5,
        // capture store); the raw store here was an uncounted store. The heap is
        // the driving VM's own, via the threaded vm pointer.
        let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
        crate::value::arena::capture_store_with_rebind(heap, slot, new_val);
    } else {
        unsafe {
            *env_ptr.add(idx) = new_val;
        }
    }
    JitValue::nil()
}

#[cfg(test)]
mod tests;
