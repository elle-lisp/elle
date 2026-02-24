//! Array operations primitives
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Create an array from arguments
pub fn prim_array(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::array(args.to_vec()))
}

/// Get the length of an array
pub fn prim_array_length(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array-length: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(v) = args[0].as_array() {
        let borrowed = v.borrow();
        (SIG_OK, Value::int(borrowed.len() as i64))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("array-length: expected array, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Get a reference from an array at an index
pub fn prim_array_ref(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array-ref: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].as_array() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-ref: expected array, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let index = match args[1].as_int() {
        Some(i) => i as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-ref: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let borrowed = vec.borrow();
    match borrowed.get(index).cloned() {
        Some(v) => (SIG_OK, v),
        None => (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "array-ref: index {} out of bounds (length {})",
                    index,
                    borrowed.len()
                ),
            ),
        ),
    }
}

/// Set a value in an array at an index (returns new array)
pub fn prim_array_set(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array-set!: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let vec_ref = match args[0].as_array() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-set!: expected array, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let index = match args[1].as_int() {
        Some(i) => i as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-set!: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let value = args[2];

    let mut vec = vec_ref.borrow_mut();
    if index >= vec.len() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "array-set!: index {} out of bounds (length {})",
                    index,
                    vec.len()
                ),
            ),
        );
    }

    vec[index] = value;
    drop(vec);
    (SIG_OK, Value::array(vec_ref.borrow().clone()))
}
