//! Data structure and cell helpers for JIT-compiled code

use crate::value::repr::TAG_NIL;
use crate::value::Value;

// =============================================================================
// Data Construction
// =============================================================================

/// Allocate a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cons(car: u64, cdr: u64) -> u64 {
    let car = unsafe { Value::from_bits(car) };
    let cdr = unsafe { Value::from_bits(cdr) };
    Value::cons(car, cdr).to_bits()
}

/// Extract car from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_car(pair_bits: u64) -> u64 {
    let pair = unsafe { Value::from_bits(pair_bits) };
    match pair.as_cons() {
        Some(cons) => cons.first.to_bits(),
        None => super::runtime::elle_jit_type_error_str("pair"),
    }
}

/// Extract cdr from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cdr(pair_bits: u64) -> u64 {
    let pair = unsafe { Value::from_bits(pair_bits) };
    match pair.as_cons() {
        Some(cons) => cons.rest.to_bits(),
        None => super::runtime::elle_jit_type_error_str("pair"),
    }
}

/// Allocate an array from a list of elements
#[no_mangle]
pub extern "C" fn elle_jit_make_array(elements: *const u64, count: u32) -> u64 {
    let mut vec = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let bits = unsafe { *elements.add(i) };
        vec.push(unsafe { Value::from_bits(bits) });
    }
    Value::array_mut(vec).to_bits()
}

/// Check if value is a pair (cons cell)
#[no_mangle]
pub extern "C" fn elle_jit_is_pair(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_cons()).to_bits()
}

/// Check if value is an immutable array
#[no_mangle]
pub extern "C" fn elle_jit_is_array(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_array()).to_bits()
}

/// Check if value is a mutable @array
#[no_mangle]
pub extern "C" fn elle_jit_is_array_mut(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_array_mut()).to_bits()
}

/// Check if value is an immutable struct
#[no_mangle]
pub extern "C" fn elle_jit_is_struct(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_struct()).to_bits()
}

/// Check if value is a mutable @struct
#[no_mangle]
pub extern "C" fn elle_jit_is_struct_mut(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_struct_mut()).to_bits()
}

/// Check if value is an immutable set
#[no_mangle]
pub extern "C" fn elle_jit_is_set(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_set()).to_bits()
}

/// Check if value is a mutable @set
#[no_mangle]
pub extern "C" fn elle_jit_is_set_mut(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_set_mut()).to_bits()
}

/// Car for destructuring: returns car if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_car_destructure(a: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    match val.as_cons() {
        Some(cons) => cons.first.to_bits(),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.fiber.signal = Some((
                crate::value::SIG_ERROR,
                crate::value::error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
            crate::value::repr::TAG_NIL
        }
    }
}

/// Cdr for destructuring: returns cdr if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_cdr_destructure(a: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    match val.as_cons() {
        Some(cons) => cons.rest.to_bits(),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.fiber.signal = Some((
                crate::value::SIG_ERROR,
                crate::value::error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
            crate::value::repr::TAG_NIL
        }
    }
}

/// Array ref for destructuring: signals error if out of bounds or not an array.
#[no_mangle]
pub extern "C" fn elle_jit_array_ref_destructure(a: u64, index: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    let idx = index as usize;
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        match borrowed.get(idx).copied() {
            Some(v) => v.to_bits(),
            None => {
                vm_ref.fiber.signal = Some((
                    crate::value::SIG_ERROR,
                    crate::value::error_val(
                        "type-error",
                        format!(
                            "destructuring: array index {} out of bounds (length {})",
                            idx,
                            borrowed.len()
                        ),
                    ),
                ));
                crate::value::repr::TAG_NIL
            }
        }
    } else if let Some(elems) = val.as_array() {
        match elems.get(idx).copied() {
            Some(v) => v.to_bits(),
            None => {
                vm_ref.fiber.signal = Some((
                    crate::value::SIG_ERROR,
                    crate::value::error_val(
                        "type-error",
                        format!(
                            "destructuring: array index {} out of bounds (length {})",
                            idx,
                            elems.len()
                        ),
                    ),
                ));
                crate::value::repr::TAG_NIL
            }
        }
    } else {
        vm_ref.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            ),
        ));
        crate::value::repr::TAG_NIL
    }
}

/// Array slice from index: returns sub-array from index to end, preserving mutability.
/// Signals error if not an array.
#[no_mangle]
pub extern "C" fn elle_jit_array_slice_from(a: u64, index: u64, vm: *mut ()) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    let idx = index as usize;
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        let slice = if idx < borrowed.len() {
            borrowed[idx..].to_vec()
        } else {
            vec![]
        };
        Value::array_mut(slice).to_bits()
    } else if let Some(elems) = val.as_array() {
        let slice = if idx < elems.len() {
            elems[idx..].to_vec()
        } else {
            vec![]
        };
        Value::array(slice).to_bits()
    } else {
        let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
        vm_ref.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            ),
        ));
        crate::value::repr::TAG_NIL
    }
}

/// Car with silent nil: returns car if cons, NIL otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_car_or_nil(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    match val.as_cons() {
        Some(cons) => cons.first.to_bits(),
        None => Value::NIL.to_bits(),
    }
}

/// Cdr with silent empty-list: returns cdr if cons, EMPTY_LIST otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_cdr_or_nil(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    match val.as_cons() {
        Some(cons) => cons.rest.to_bits(),
        None => Value::EMPTY_LIST.to_bits(),
    }
}

/// Array length: returns length as int for array or @array, 0 otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_array_len(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    let len = if let Some(arr) = val.as_array_mut() {
        arr.borrow().len() as i64
    } else if let Some(arr) = val.as_array() {
        arr.len() as i64
    } else {
        0
    };
    Value::int(len).to_bits()
}

/// Array ref with silent nil: returns element at index, NIL if out of bounds or not array.
#[no_mangle]
pub extern "C" fn elle_jit_array_ref_or_nil(a: u64, index: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    let idx = index as usize;
    let result = if let Some(arr) = val.as_array_mut() {
        arr.borrow().get(idx).copied()
    } else if let Some(arr) = val.as_array() {
        arr.get(idx).copied()
    } else {
        None
    };
    result.unwrap_or(Value::NIL).to_bits()
}

// =============================================================================
// Box Operations
// =============================================================================

/// Create a LocalCell wrapping a value
#[no_mangle]
pub extern "C" fn elle_jit_make_lbox(value: u64) -> u64 {
    let val = unsafe { Value::from_bits(value) };
    Value::local_lbox(val).to_bits()
}

/// Load value from a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_load_lbox(cell_bits: u64) -> u64 {
    let cell = unsafe { Value::from_bits(cell_bits) };
    if let Some(cell_ref) = cell.as_lbox() {
        cell_ref.borrow().to_bits()
    } else {
        super::runtime::elle_jit_type_error_str("cell")
    }
}

/// Load from env slot, auto-unwrapping LocalCell if present.
/// This matches the interpreter's LoadUpvalue semantics:
/// - LocalCell (compiler-created mutable capture): unwrap and return inner value
/// - Everything else (plain value, user Cell, etc.): return as-is
#[no_mangle]
pub extern "C" fn elle_jit_load_capture(val_bits: u64) -> u64 {
    let val = unsafe { Value::from_bits(val_bits) };
    if val.is_local_lbox() {
        if let Some(cell_ref) = val.as_lbox() {
            cell_ref.borrow().to_bits()
        } else {
            val_bits // shouldn't happen, but safe fallback
        }
    } else {
        val_bits
    }
}

/// Store value into a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_store_lbox(cell_bits: u64, value: u64) -> u64 {
    let cell = unsafe { Value::from_bits(cell_bits) };
    let val = unsafe { Value::from_bits(value) };
    if let Some(cell_ref) = cell.as_lbox() {
        *cell_ref.borrow_mut() = val;
        TAG_NIL
    } else {
        super::runtime::elle_jit_type_error_str("cell")
    }
}

/// Store to a capture slot, handling cells automatically
/// If the slot contains a LocalCell, stores into the cell.
/// Otherwise, stores directly to the slot.
#[no_mangle]
pub extern "C" fn elle_jit_store_capture(env_ptr: *mut u64, index: u64, value: u64) -> u64 {
    let idx = index as usize;
    let slot_bits = unsafe { *env_ptr.add(idx) };
    let slot = unsafe { Value::from_bits(slot_bits) };

    if slot.is_local_lbox() {
        // Store into the cell
        if let Some(cell_ref) = slot.as_lbox() {
            let new_val = unsafe { Value::from_bits(value) };
            *cell_ref.borrow_mut() = new_val;
        }
    } else {
        // Direct store to the slot
        unsafe {
            *env_ptr.add(idx) = value;
        }
    }
    TAG_NIL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cons_car_cdr() {
        let car = Value::int(1).to_bits();
        let cdr = Value::int(2).to_bits();
        let pair = elle_jit_cons(car, cdr);

        let car_result = elle_jit_car(pair);
        let cdr_result = elle_jit_cdr(pair);

        let car_val = unsafe { Value::from_bits(car_result) };
        let cdr_val = unsafe { Value::from_bits(cdr_result) };

        assert_eq!(car_val.as_int(), Some(1));
        assert_eq!(cdr_val.as_int(), Some(2));
    }

    #[test]
    fn test_is_pair() {
        let pair = elle_jit_cons(Value::int(1).to_bits(), Value::int(2).to_bits());
        let is_pair = unsafe { Value::from_bits(elle_jit_is_pair(pair)) };
        assert_eq!(is_pair.as_bool(), Some(true));

        let not_pair = unsafe { Value::from_bits(elle_jit_is_pair(Value::int(42).to_bits())) };
        assert_eq!(not_pair.as_bool(), Some(false));
    }

    #[test]
    fn test_make_array() {
        let elements = [
            Value::int(1).to_bits(),
            Value::int(2).to_bits(),
            Value::int(3).to_bits(),
        ];
        let vec_bits = elle_jit_make_array(elements.as_ptr(), 3);
        let vec = unsafe { Value::from_bits(vec_bits) };

        assert!(vec.is_array_mut());
        let vec_ref = vec.as_array_mut().unwrap();
        let borrowed = vec_ref.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0].as_int(), Some(1));
        assert_eq!(borrowed[1].as_int(), Some(2));
        assert_eq!(borrowed[2].as_int(), Some(3));
    }

    #[test]
    fn test_cell_operations() {
        // Make a cell
        let cell_bits = elle_jit_make_lbox(Value::int(42).to_bits());
        let cell = unsafe { Value::from_bits(cell_bits) };
        assert!(cell.is_local_lbox());

        // Load from cell
        let loaded = elle_jit_load_lbox(cell_bits);
        let loaded_val = unsafe { Value::from_bits(loaded) };
        assert_eq!(loaded_val.as_int(), Some(42));

        // Store to cell
        elle_jit_store_lbox(cell_bits, Value::int(100).to_bits());

        // Load again
        let loaded2 = elle_jit_load_lbox(cell_bits);
        let loaded_val2 = unsafe { Value::from_bits(loaded2) };
        assert_eq!(loaded_val2.as_int(), Some(100));
    }
}
