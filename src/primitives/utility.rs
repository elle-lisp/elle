//! Utility primitives (mod, remainder, even?, odd?)
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Modulo operation (result has same sign as divisor)
pub fn prim_mod(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mod: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b == 0 {
                return (
                    SIG_ERROR,
                    error_val("division-by-zero", "mod: division by zero"),
                );
            }
            // Lisp mod: result has same sign as divisor
            let rem = a % b;
            if rem == 0 {
                (SIG_OK, Value::int(0))
            } else if (rem > 0) != (b > 0) {
                (SIG_OK, Value::int(rem + b))
            } else {
                (SIG_OK, Value::int(rem))
            }
        }
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("mod: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Remainder operation (result has same sign as dividend)
pub fn prim_remainder(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("remainder: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b == 0 {
                return (
                    SIG_ERROR,
                    error_val("division-by-zero", "remainder: division by zero"),
                );
            }
            let rem = a % b;
            // Adjust remainder to have same sign as dividend
            if (rem > 0 && b < 0) || (rem < 0 && b > 0) {
                (SIG_OK, Value::int(rem + b))
            } else {
                (SIG_OK, Value::int(rem))
            }
        }
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("remainder: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Check if number is even
pub fn prim_even(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("even?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::bool(n % 2 == 0)),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("even?: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Check if number is odd
pub fn prim_odd(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("odd?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::bool(n % 2 != 0)),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("odd?: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}
