// audited: 2026-10-22
//! The data opcodes: pairs and their parts, array construction and indexing, and the destructuring reads.
//!
//! docs/impl/vm.md

use super::core::VM;
use crate::hir::region::RuntimeRegion;
use crate::value::{sorted_struct_get, TableKey, Value, SIG_ERROR};

mod structops;
pub(crate) use structops::*;

pub(crate) fn handle_list(vm: &mut VM, region_id: RuntimeRegion) {
    let rest = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Pair");
    let first = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Pair");
    // Containment of `first`/`rest`'s cross-region members is counted and
    // recorded exactly once by the alloc funnel: `alloc_in_region` → `alloc_obj`
    // → `incref_cross_region_refs` scans the object, increfs each cross-region
    // ref, and records the outgoing edge the free-time cascade decrefs. Increffing
    // here in addition would count each member twice against that single cascade
    // decref — a per-op over-keep of every heap element stored into a cons
    // (pinned by the `arg-result` and `take` oracle probes). The same
    // alloc-funnel-only discipline `args_to_list` and every native list/array
    // constructor uses; soundness pinned by region-pair-heap-content-uaf.lisp.
    let val = crate::value::build::pair(vm.heap(), first, rest, region_id);
    vm.fiber.stack.push(val);
}

pub(crate) fn handle_first(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on First");

    // car of nil is an error - enforces proper list invariant
    if val.is_nil() {
        vm.set_error("type-error", "first: cannot take first of nil");
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // car of empty list is an error
    if val.is_empty_list() {
        vm.set_error("type-error", "first: cannot take first of empty list");
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // Handle pair cells
    if let Some(pair) = val.as_pair() {
        vm.fiber.stack.push(pair.first);
    } else {
        vm.set_error(
            "type-error",
            format!("first: expected pair, got {}", val.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
    }
}

pub(crate) fn handle_rest(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Rest");

    // cdr of nil is an error - enforces proper list invariant
    if val.is_nil() {
        vm.set_error("type-error", "rest: cannot take rest of nil");
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // cdr of empty list is an error
    if val.is_empty_list() {
        vm.set_error("type-error", "rest: cannot take rest of empty list");
        vm.fiber.stack.push(Value::NIL);
        return;
    }

    // Handle pair cells
    if let Some(pair) = val.as_pair() {
        vm.fiber.stack.push(pair.rest);
    } else {
        vm.set_error(
            "type-error",
            format!("rest: expected pair, got {}", val.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
    }
}

pub(crate) fn handle_make_array(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    region_id: RuntimeRegion,
) {
    let size = vm.read_u8(bytecode, ip) as usize;
    let mut vec = Vec::with_capacity(size);
    for _ in 0..size {
        vec.push(
            vm.fiber
                .stack
                .pop()
                .expect("VM bug: Stack underflow on MakeArrayMut"),
        );
    }
    vec.reverse();
    // Element containment is counted+recorded once by the alloc funnel
    // (`alloc_in_region` → `incref_cross_region_refs`), balanced by the free-time
    // cascade — see `handle_list` for the full note. A manual pre-incref here
    // would double-count each cross-region element against that single cascade
    // decref.
    let val = crate::value::build::array_mut(vm.heap(), vec, region_id);
    vm.fiber.stack.push(val);
}

pub(crate) fn handle_array_ref(vm: &mut VM) {
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRef");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRef");
    let Some(idx_val) = idx.as_int() else {
        vm_type_error!(vm, idx, "array-ref", "integer index")
    };
    let Some(elems) = vec.array_mut_read() else {
        vm_type_error!(vm, vec, "array-ref", "array")
    };
    match elems.get(idx_val as usize) {
        Some(val) => {
            vm.fiber.stack.push(*val);
        }
        None => {
            let len = elems.len();
            drop(elems);
            vm.set_error(
                "argument-error",
                format!(
                    "array-ref: index {} out of bounds (length {})",
                    idx_val, len
                ),
            );
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

pub(crate) fn handle_array_set(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
    let idx = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
    let vec = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSet");
    let Some(_idx_val) = idx.as_int() else {
        vm_type_error!(vm, idx, "array-set!", "integer index")
    };
    if !vec.is_array_mut() {
        vm_type_error!(vm, vec, "array-set!", "array")
    }
    // No emitter produces ArrayMutSet: the handler checks its operands and
    // stores nothing.
    vm.fiber.stack.push(val);
}

/// No match arm covered the scrutinee: signals :match-error carrying it.
pub(crate) fn handle_match_fail(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on MatchFail");
    let err = vm.escaping_match_fail(val);
    vm.fiber.signal = Some((SIG_ERROR, err));
    vm.fiber.stack.push(Value::NIL);
}

/// First for destructuring: signals error if not a pair cell.
pub(crate) fn handle_car_destructure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on FirstDestructure");
    match val.as_pair() {
        Some(pair) => vm.fiber.stack.push(pair.first),
        None => {
            vm.set_error(
                "type-error",
                format!("destructuring: expected list, got {}", val.type_name()),
            );
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

/// Rest for destructuring: signals error if not a pair cell.
pub(crate) fn handle_cdr_destructure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on RestDestructure");
    match val.as_pair() {
        Some(pair) => vm.fiber.stack.push(pair.rest),
        None => {
            vm.set_error(
                "type-error",
                format!("destructuring: expected list, got {}", val.type_name()),
            );
            vm.fiber.stack.push(Value::EMPTY_LIST);
        }
    }
}

/// Array ref for destructuring: signals error if not an array or out of bounds.
/// Operand: u16 index (immediate, read from bytecode).
pub(crate) fn handle_array_ref_destructure(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutRefDestructure");
    let found = match val.array_mut_read() {
        Some(read) => Some((read.get(index).copied(), read.len())),
        None => val
            .as_array()
            .map(|elems| (elems.get(index).copied(), elems.len())),
    };
    match found {
        Some((Some(v), _)) => vm.fiber.stack.push(v),
        Some((None, len)) => {
            vm.set_error(
                "type-error",
                format!("destructuring: array index {index} out of bounds (length {len})"),
            );
            vm.fiber.stack.push(Value::NIL);
        }
        None => {
            vm.set_error(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            );
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

/// Array slice from index for destructuring: returns sub-array from index to end.
/// Works on both arrays and @arrays; result type matches input type.
/// Empty slice (index >= length) is valid. Signals error on wrong type.
/// Operand: u16 index (immediate, read from bytecode).
/// Used by & rest destructuring.
pub(crate) fn handle_array_slice_from(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let index = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutSliceFrom");
    // The fresh sub-array is the destructure-rest binding's value: born in its
    // own fresh region (Rule 3; docs/impl/region/ctx.md), freed
    // value-based by the binding's consumer. A borrowing structural opcode that
    // the solver assigns no region slot, so it builds a `NativeCtx` over a
    // freshly minted region here, mirroring the native-result discipline.
    let tail = match val.array_mut_read() {
        Some(read) => Some((read.get(index..).unwrap_or_default().to_vec(), true)),
        None => val
            .as_array()
            .map(|elems| (elems.get(index..).unwrap_or_default().to_vec(), false)),
    };
    let Some((elems, mutable)) = tail else {
        vm_type_error!(vm, val, "destructuring", "array")
    };
    let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
    let result = if mutable {
        ctx.array_mut(elems)
    } else {
        ctx.array(elems)
    };
    vm.fiber.stack.push(result);
}
