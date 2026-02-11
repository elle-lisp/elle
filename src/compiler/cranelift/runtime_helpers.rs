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
fn encode_value_for_jit(value: &Value) -> i64 {
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
