//! Data structure and cell helpers for JIT-compiled code

use crate::jit::value::JitValue;
use crate::value::Value;

// =============================================================================
// Data Construction
// =============================================================================

/// Allocate a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cons(
    car_tag: u64,
    car_payload: u64,
    cdr_tag: u64,
    cdr_payload: u64,
) -> JitValue {
    let car = Value {
        tag: car_tag,
        payload: car_payload,
    };
    let cdr = Value {
        tag: cdr_tag,
        payload: cdr_payload,
    };
    JitValue::from_value(Value::cons(car, cdr))
}

/// Extract car from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_car(pair_tag: u64, pair_payload: u64) -> JitValue {
    let pair = Value {
        tag: pair_tag,
        payload: pair_payload,
    };
    match pair.as_cons() {
        Some(cons) => JitValue::from_value(cons.first),
        None => {
            eprintln!("JIT type error: expected pair");
            JitValue::nil()
        }
    }
}

/// Extract cdr from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cdr(pair_tag: u64, pair_payload: u64) -> JitValue {
    let pair = Value {
        tag: pair_tag,
        payload: pair_payload,
    };
    match pair.as_cons() {
        Some(cons) => JitValue::from_value(cons.rest),
        None => {
            eprintln!("JIT type error: expected pair");
            JitValue::nil()
        }
    }
}

/// Allocate an array from a list of elements
/// elements: *const Value (16 bytes each)
#[no_mangle]
pub extern "C" fn elle_jit_make_array(elements: *const Value, count: u64) -> JitValue {
    let count = count as usize;
    let mut vec = Vec::with_capacity(count);
    for i in 0..count {
        let v = unsafe { *elements.add(i) };
        vec.push(v);
    }
    JitValue::from_value(Value::array_mut(vec))
}

/// Check if value is a pair (cons cell)
#[no_mangle]
pub extern "C" fn elle_jit_is_pair(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::bool_val(val.is_cons())
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

/// Car for destructuring: returns car if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_car_destructure(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    match val.as_cons() {
        Some(cons) => JitValue::from_value(cons.first),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.fiber.signal = Some((
                crate::value::SIG_ERROR,
                crate::value::error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
            JitValue::nil()
        }
    }
}

/// Cdr for destructuring: returns cdr if cons, signals error otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_cdr_destructure(tag: u64, payload: u64, vm: *mut ()) -> JitValue {
    let val = Value { tag, payload };
    match val.as_cons() {
        Some(cons) => JitValue::from_value(cons.rest),
        None => {
            let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
            vm.fiber.signal = Some((
                crate::value::SIG_ERROR,
                crate::value::error_val(
                    "type-error",
                    format!("destructuring: expected list, got {}", val.type_name()),
                ),
            ));
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
                JitValue::nil()
            }
        }
    } else if let Some(elems) = val.as_array() {
        match elems.get(idx).copied() {
            Some(v) => JitValue::from_value(v),
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
                JitValue::nil()
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
    let val = Value { tag, payload };
    let idx = index as usize;
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        let slice = if idx < borrowed.len() {
            borrowed[idx..].to_vec()
        } else {
            vec![]
        };
        JitValue::from_value(Value::array_mut(slice))
    } else if let Some(elems) = val.as_array() {
        let slice = if idx < elems.len() {
            elems[idx..].to_vec()
        } else {
            vec![]
        };
        JitValue::from_value(Value::array(slice))
    } else {
        let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
        vm_ref.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val(
                "type-error",
                format!("destructuring: expected array, got {}", val.type_name()),
            ),
        ));
        JitValue::nil()
    }
}

/// Car with silent nil: returns car if cons, NIL otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_car_or_nil(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    match val.as_cons() {
        Some(cons) => JitValue::from_value(cons.first),
        None => JitValue::nil(),
    }
}

/// Cdr with silent empty-list: returns cdr if cons, EMPTY_LIST otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_cdr_or_nil(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    match val.as_cons() {
        Some(cons) => JitValue::from_value(cons.rest),
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
pub extern "C" fn elle_jit_make_lbox(tag: u64, payload: u64) -> JitValue {
    let val = Value { tag, payload };
    JitValue::from_value(Value::local_lbox(val))
}

/// Load value from a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_load_lbox(cell_tag: u64, cell_payload: u64) -> JitValue {
    let cell = Value {
        tag: cell_tag,
        payload: cell_payload,
    };
    if let Some(cell_ref) = cell.as_lbox() {
        JitValue::from_value(*cell_ref.borrow())
    } else {
        eprintln!("JIT type error: expected cell");
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
    if val.is_local_lbox() {
        if let Some(cell_ref) = val.as_lbox() {
            JitValue::from_value(*cell_ref.borrow())
        } else {
            JitValue { tag, payload } // shouldn't happen, but safe fallback
        }
    } else {
        JitValue { tag, payload }
    }
}

/// Store value into a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_store_lbox(
    cell_tag: u64,
    cell_payload: u64,
    val_tag: u64,
    val_payload: u64,
) -> JitValue {
    let cell = Value {
        tag: cell_tag,
        payload: cell_payload,
    };
    let val = Value {
        tag: val_tag,
        payload: val_payload,
    };
    if let Some(cell_ref) = cell.as_lbox() {
        *cell_ref.borrow_mut() = val;
    } else {
        eprintln!("JIT type error: expected cell");
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
) -> JitValue {
    let idx = index as usize;
    let slot = unsafe { *env_ptr.add(idx) };
    let new_val = Value {
        tag: val_tag,
        payload: val_payload,
    };

    if slot.is_local_lbox() {
        if let Some(cell_ref) = slot.as_lbox() {
            *cell_ref.borrow_mut() = new_val;
        }
    } else {
        unsafe {
            *env_ptr.add(idx) = new_val;
        }
    }
    JitValue::nil()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cons_car_cdr() {
        let car = Value::int(1);
        let cdr = Value::int(2);
        let pair = elle_jit_cons(car.tag, car.payload, cdr.tag, cdr.payload).to_value();

        let car_val = elle_jit_car(pair.tag, pair.payload).to_value();
        let cdr_val = elle_jit_cdr(pair.tag, pair.payload).to_value();

        assert_eq!(car_val.as_int(), Some(1));
        assert_eq!(cdr_val.as_int(), Some(2));
    }

    #[test]
    fn test_is_pair() {
        let car = Value::int(1);
        let cdr = Value::int(2);
        let pair = elle_jit_cons(car.tag, car.payload, cdr.tag, cdr.payload).to_value();

        assert_eq!(
            elle_jit_is_pair(pair.tag, pair.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_is_pair(Value::int(42).tag, Value::int(42).payload),
            JitValue::bool_val(false)
        );
    }

    #[test]
    fn test_make_array() {
        let elements = [Value::int(1), Value::int(2), Value::int(3)];
        let vec_val = elle_jit_make_array(elements.as_ptr(), 3).to_value();

        assert!(vec_val.is_array_mut());
        let vec_ref = vec_val.as_array_mut().unwrap();
        let borrowed = vec_ref.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0].as_int(), Some(1));
        assert_eq!(borrowed[1].as_int(), Some(2));
        assert_eq!(borrowed[2].as_int(), Some(3));
    }

    #[test]
    fn test_cell_operations() {
        let v = Value::int(42);
        let cell = elle_jit_make_lbox(v.tag, v.payload).to_value();
        assert!(cell.is_local_lbox());

        let loaded = elle_jit_load_lbox(cell.tag, cell.payload).to_value();
        assert_eq!(loaded.as_int(), Some(42));

        let new_val = Value::int(100);
        elle_jit_store_lbox(cell.tag, cell.payload, new_val.tag, new_val.payload);

        let loaded2 = elle_jit_load_lbox(cell.tag, cell.payload).to_value();
        assert_eq!(loaded2.as_int(), Some(100));
    }
}
