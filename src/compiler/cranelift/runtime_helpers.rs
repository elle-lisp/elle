//! Runtime helper functions for JIT-compiled code
//!
//! These functions are called from generated native code to perform
//! operations that require access to Elle's runtime (heap values, etc.)

use crate::value::Value;
use std::cell::RefCell;

thread_local! {
    /// Pinned values that must not be collected during JIT execution
    static PINNED_VALUES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

/// Pin a value to prevent collection during JIT execution
/// Returns an i64 pointer to the pinned value
pub fn pin_value(value: Value) -> i64 {
    PINNED_VALUES.with(|pinned| {
        let mut vec = pinned.borrow_mut();
        vec.push(value);
        let ptr = vec.last().unwrap() as *const Value;
        ptr as i64
    })
}

/// Unpin all values after JIT execution completes
pub fn unpin_all() {
    PINNED_VALUES.with(|pinned| {
        pinned.borrow_mut().clear();
    });
}

/// Check if a value is nil
/// Returns 1 for nil, 0 otherwise
///
/// # Safety
/// The value_ptr must be either 0 (encoded nil) or a valid pointer to a Value
#[no_mangle]
pub extern "C" fn jit_is_nil(value_ptr: i64) -> i64 {
    if value_ptr == 0 {
        return 1; // Encoded nil
    }
    let value = unsafe { &*(value_ptr as *const Value) };
    if value.is_nil() {
        1
    } else {
        0
    }
}

/// Extract the car (first element) of a cons cell
/// Returns an i64-encoded value or pointer
///
/// # Safety
/// The value_ptr must be either 0 or a valid pointer to a Value
#[no_mangle]
pub extern "C" fn jit_car(value_ptr: i64) -> i64 {
    if value_ptr == 0 {
        return 0; // car of nil is nil
    }
    let value = unsafe { &*(value_ptr as *const Value) };
    match value {
        Value::Cons(cons) => encode_value_for_jit(&cons.first),
        _ => 0, // car of non-cons is nil
    }
}

/// Extract the cdr (rest) of a cons cell
/// Returns an i64-encoded value or pointer
///
/// # Safety
/// The value_ptr must be either 0 or a valid pointer to a Value
#[no_mangle]
pub extern "C" fn jit_cdr(value_ptr: i64) -> i64 {
    if value_ptr == 0 {
        return 0; // cdr of nil is nil
    }
    let value = unsafe { &*(value_ptr as *const Value) };
    match value {
        Value::Cons(cons) => encode_value_for_jit(&cons.rest),
        _ => 0, // cdr of non-cons is nil
    }
}

/// Encode a Value as an i64 for JIT use
/// Primitives are encoded directly, heap values return pointers
pub fn encode_value_for_jit(value: &Value) -> i64 {
    match value {
        Value::Nil => 0,
        Value::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::Int(i) => *i,
        // For heap values (cons, etc.), return pointer to the value
        _ => value as *const Value as i64,
    }
}

/// Decode an i64 back to a Value for JIT use
/// This is the inverse of encode_value_for_jit
pub fn decode_value_for_jit(encoded: i64) -> Value {
    if encoded == 0 {
        Value::Nil
    } else if encoded == 1 {
        Value::Bool(true)
    } else if encoded > 1 && encoded < i64::MAX / 2 {
        // Likely a small integer
        Value::Int(encoded)
    } else {
        // Likely a pointer to a heap value
        unsafe {
            let ptr = encoded as *const Value;
            if ptr.is_null() {
                Value::Nil
            } else {
                (*ptr).clone()
            }
        }
    }
}

/// Load a global variable by symbol ID
/// Returns the encoded value, or 0 (nil) if not found
///
/// # Safety
/// Requires VM context to be set via set_vm_context
#[no_mangle]
pub extern "C" fn jit_load_global(sym_id: i64) -> i64 {
    let vm_ptr = match crate::ffi::primitives::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            eprintln!("jit_load_global: VM context not set");
            return 0;
        }
    };
    let vm = unsafe { &*vm_ptr };

    let sym_id_u32 = sym_id as u32;

    // Check scope stack first (for proper shadowing)
    if let Some(val) = vm.scope_stack.get(sym_id_u32) {
        // Handle cells (for mutable captures)
        match val {
            Value::Cell(cell_rc) | Value::LocalCell(cell_rc) => {
                let cell_ref = cell_rc.borrow();
                return encode_value_for_jit(&cell_ref);
            }
            _ => return encode_value_for_jit(&val),
        }
    }

    // Fall back to global scope
    if let Some(val) = vm.globals.get(&sym_id_u32) {
        match val {
            Value::Cell(cell_rc) | Value::LocalCell(cell_rc) => {
                let cell_ref = cell_rc.borrow();
                encode_value_for_jit(&cell_ref)
            }
            _ => encode_value_for_jit(val),
        }
    } else {
        eprintln!("jit_load_global: Undefined global variable: {}", sym_id_u32);
        0 // nil
    }
}

/// Runtime helper for JIT tail calls to closures
/// This function is called via return_call_indirect from JIT code
///
/// # Safety
/// - callee_encoded must be a valid encoded Value (closure)
/// - args_ptr must be a valid pointer to an array of args_len i64 values
#[no_mangle]
pub unsafe extern "C" fn jit_tail_call_closure(
    callee_encoded: i64,
    args_ptr: *const i64,
    args_len: i64,
) -> i64 {
    // Decode callee
    let callee = decode_value_for_jit(callee_encoded);

    // Decode arguments
    let args: Vec<Value> = if args_len > 0 && !args_ptr.is_null() {
        (0..args_len as usize)
            .map(|i| decode_value_for_jit(*args_ptr.add(i)))
            .collect()
    } else {
        Vec::new()
    };

    // Dispatch based on callee type
    match &callee {
        Value::JitClosure(jc) if !jc.code_ptr.is_null() => {
            // Native JIT closure - call directly
            // The signature is: fn(args_ptr: *const i64, args_len: i64, env_ptr: *const i64) -> i64
            let func: extern "C" fn(*const i64, i64, *const i64) -> i64 =
                unsafe { std::mem::transmute(jc.code_ptr) };

            // Encode args
            let encoded_args: Vec<i64> = args.iter().map(encode_value_for_jit).collect();

            // Encode env
            let encoded_env: Vec<i64> = jc.env.iter().map(encode_value_for_jit).collect();

            func(encoded_args.as_ptr(), args_len, encoded_env.as_ptr()) // Already encoded
        }
        Value::Closure(_c) => {
            // For interpreted closures, we would need access to the VM
            // For now, return an error encoded as nil
            eprintln!("jit_tail_call_closure: Cannot tail-call interpreted closure from JIT");
            0 // nil
        }
        Value::JitClosure(jc) => {
            // JitClosure without code_ptr - fall back to source
            if let Some(ref _source) = jc.source {
                eprintln!("jit_tail_call_closure: Cannot tail-call closure without code_ptr");
                0 // nil
            } else {
                eprintln!("jit_tail_call_closure: JitClosure has no code and no source");
                0 // nil
            }
        }
        _ => {
            eprintln!(
                "jit_tail_call_closure: Cannot tail-call non-closure: {:?}",
                callee
            );
            0 // nil
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::cons;

    #[test]
    fn test_jit_is_nil_with_zero() {
        assert_eq!(jit_is_nil(0), 1);
    }

    #[test]
    fn test_jit_is_nil_with_nil_value() {
        let nil = Value::Nil;
        let ptr = &nil as *const Value as i64;
        assert_eq!(jit_is_nil(ptr), 1);
    }

    #[test]
    fn test_jit_is_nil_with_cons() {
        let list = cons(Value::Int(1), Value::Nil);
        let ptr = &list as *const Value as i64;
        assert_eq!(jit_is_nil(ptr), 0);
    }

    #[test]
    fn test_jit_car_of_nil() {
        assert_eq!(jit_car(0), 0);
    }

    #[test]
    fn test_jit_car_of_cons() {
        let list = cons(Value::Int(42), Value::Nil);
        let ptr = &list as *const Value as i64;
        // car should return 42 (the integer value directly)
        assert_eq!(jit_car(ptr), 42);
    }

    #[test]
    fn test_jit_cdr_of_nil() {
        assert_eq!(jit_cdr(0), 0);
    }

    #[test]
    fn test_jit_cdr_of_single_element_list() {
        let list = cons(Value::Int(1), Value::Nil);
        let ptr = &list as *const Value as i64;
        // cdr should return 0 (nil)
        assert_eq!(jit_cdr(ptr), 0);
    }

    #[test]
    fn test_jit_cdr_of_multi_element_list() {
        let list = cons(Value::Int(1), cons(Value::Int(2), Value::Nil));
        let ptr = &list as *const Value as i64;
        let cdr_ptr = jit_cdr(ptr);
        // cdr should be a pointer to the rest of the list
        assert_ne!(cdr_ptr, 0);
        // The car of the cdr should be 2
        assert_eq!(jit_car(cdr_ptr), 2);
    }

    #[test]
    fn test_pin_and_unpin() {
        let value = Value::Int(42);
        let ptr = pin_value(value);
        assert_ne!(ptr, 0);

        // Value should be accessible
        let pinned = unsafe { &*(ptr as *const Value) };
        assert_eq!(pinned.as_int().unwrap(), 42);

        unpin_all();
    }
}
