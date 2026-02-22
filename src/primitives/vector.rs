//! Vector operations primitives
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Create a vector from arguments
pub fn prim_vector(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::vector(args.to_vec()))
}

/// Get the length of a vector
pub fn prim_vector_length(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vector-length: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(v) = args[0].as_vector() {
        let borrowed = v.borrow();
        (SIG_OK, Value::int(borrowed.len() as i64))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "vector-length: expected vector, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Get a reference from a vector at an index
pub fn prim_vector_ref(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vector-ref: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].as_vector() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("vector-ref: expected vector, got {}", args[0].type_name()),
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
                    format!("vector-ref: expected integer, got {}", args[1].type_name()),
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
                    "vector-ref: index {} out of bounds (length {})",
                    index,
                    borrowed.len()
                ),
            ),
        ),
    }
}

/// Set a value in a vector at an index (returns new vector)
pub fn prim_vector_set(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vector-set!: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let vec_ref = match args[0].as_vector() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("vector-set!: expected vector, got {}", args[0].type_name()),
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
                    format!("vector-set!: expected integer, got {}", args[1].type_name()),
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
                    "vector-set!: index {} out of bounds (length {})",
                    index,
                    vec.len()
                ),
            ),
        );
    }

    vec[index] = value;
    drop(vec);
    (SIG_OK, Value::vector(vec_ref.borrow().clone()))
}
