//! Capture-cell (box) operations for JIT-compiled code.
//!
//! These mint and access the `CaptureCell`s that back mutable captured locals,
//! plus the rest-arg list builder. The `_owned` and `collect_rest_list`
//! prologue paths deliberately mint their OWN per-execution region rather than
//! allocating into the caller's inherited region — see each function's WHY for
//! the Rule-6/Rule-8 leak/use-after-free they avoid.

use super::region_of_raw;
use crate::jit::value::JitValue;
use crate::value::Value;

/// Create a LocalCell wrapping a value
#[no_mangle]
pub extern "C" fn elle_jit_make_capture(
    tag: u64,
    payload: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    let val = Value { tag, payload };
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region/ctx.md).
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
/// allocated there commingles with it (docs/impl/region/rules.md Rule 6) and its
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
    // (docs/impl/region/ctx.md).
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
