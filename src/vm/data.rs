use super::core::VM;
use crate::hir::region::RuntimeRegion;
use crate::value::{sorted_struct_get, TableKey, Value, SIG_ERROR};

mod structops;
pub(crate) use structops::*;

pub(crate) fn handle_list(vm: &mut VM, region_id: RuntimeRegion) {
    use crate::value::heap::{HeapObject, HeapTag, Pair};
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
    incref_cross_region(vm, first, region_id);
    incref_cross_region(vm, rest, region_id);
    let traits = crate::primitives::traitregistry::default_traits_for(
        unsafe { &*vm.heap_ptr },
        HeapTag::Pair,
    );
    let obj = HeapObject::Pair(Pair {
        first,
        rest,
        traits,
    });
    let val = vm.heap().alloc_in_region(obj, region_id);
    vm.fiber.stack.push(val);
}

/// Incref the region of `val` if it's a heap value in a different region
/// than `target_region`. Balances the cascade decref in `free_runtime_region_pages`.
fn incref_cross_region(vm: &mut VM, val: Value, target_region: RuntimeRegion) {
    let heap = unsafe { &mut *vm.heap_ptr };
    if let Some(rid) = crate::value::arena::region_of(heap, val) {
        // Skip a self-edge (val already lives in the container's region).
        if rid != target_region {
            crate::value::arena::incref_for_escape(
                heap,
                Some(rid),
                crate::value::arena::EscapeSite::ImmutableContents,
            );
        }
    }
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
    use crate::value::heap::{HeapObject, HeapTag};
    use std::cell::RefCell;
    use std::rc::Rc;
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
    for elem in &vec {
        incref_cross_region(vm, *elem, region_id);
    }
    let traits = crate::primitives::traitregistry::default_traits_for(
        unsafe { &*vm.heap_ptr },
        HeapTag::LArrayMut,
    );
    let obj = HeapObject::LArrayMut {
        data: Rc::new(RefCell::new(vec)),
        traits,
    };
    let val = vm.heap().alloc_in_region(obj, region_id);
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
        vm.set_error(
            "type-error",
            format!("array-ref: expected integer index, got {}", idx.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    let Some(vec_ref) = vec.as_array_mut() else {
        vm.set_error(
            "type-error",
            format!("array-ref: expected array, got {}", vec.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    let vec_borrow = vec_ref.borrow();
    match vec_borrow.get(idx_val as usize) {
        Some(val) => {
            vm.fiber.stack.push(*val);
        }
        None => {
            let len = vec_borrow.len();
            drop(vec_borrow);
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
        vm.set_error(
            "type-error",
            format!(
                "array-set!: expected integer index, got {}",
                idx.type_name()
            ),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    if !vec.is_array_mut() {
        vm.set_error(
            "type-error",
            format!("array-set!: expected array, got {}", vec.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    // Note: Arrays are immutable in this implementation
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
    if let Some(vec_ref) = val.as_array_mut() {
        let borrowed = vec_ref.borrow();
        match borrowed.get(index).copied() {
            Some(v) => vm.fiber.stack.push(v),
            None => {
                let len = borrowed.len();
                drop(borrowed);
                vm.set_error(
                    "type-error",
                    format!(
                        "destructuring: array index {} out of bounds (length {})",
                        index, len
                    ),
                );
                vm.fiber.stack.push(Value::NIL);
            }
        }
    } else if let Some(elems) = val.as_array() {
        match elems.get(index).copied() {
            Some(v) => vm.fiber.stack.push(v),
            None => {
                let len = elems.len();
                vm.set_error(
                    "type-error",
                    format!(
                        "destructuring: array index {} out of bounds (length {})",
                        index, len
                    ),
                );
                vm.fiber.stack.push(Value::NIL);
            }
        }
    } else {
        vm.set_error(
            "type-error",
            format!("destructuring: expected array, got {}", val.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
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
    // own fresh region (Rule 3; docs/impl/region-ctx.md), freed
    // value-based by the binding's consumer. A borrowing structural opcode that
    // the solver assigns no region slot, so it builds a `NativeCtx` over a
    // freshly minted region here, mirroring the native-result discipline.
    let result = if let Some(vec_ref) = val.as_array_mut() {
        let borrowed = vec_ref.borrow();
        let elems = if index < borrowed.len() {
            borrowed[index..].to_vec()
        } else {
            vec![]
        };
        drop(borrowed);
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        ctx.array_mut(elems)
    } else if let Some(elems) = val.as_array() {
        let elems = if index < elems.len() {
            elems[index..].to_vec()
        } else {
            vec![]
        };
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        ctx.array(elems)
    } else {
        vm.set_error(
            "type-error",
            format!("destructuring: expected array, got {}", val.type_name()),
        );
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    vm.fiber.stack.push(result);
}
