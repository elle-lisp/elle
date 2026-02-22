use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Logical NOT operation
pub fn prim_not(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("not: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(!args[0].is_truthy()))
}

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub fn prim_and(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(true));
    }

    for arg in &args[..args.len() - 1] {
        if !arg.is_truthy() {
            return (SIG_OK, *arg);
        }
    }

    (SIG_OK, args[args.len() - 1])
}

/// Logical OR operation
/// (or) => false
/// (or x) => x
/// (or x y z) => x if truthy, else next truthy or z
pub fn prim_or(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(false));
    }

    for arg in &args[..args.len() - 1] {
        if arg.is_truthy() {
            return (SIG_OK, *arg);
        }
    }

    (SIG_OK, args[args.len() - 1])
}

/// Logical XOR operation
/// (xor) => false
/// (xor x) => x (as bool)
/// (xor x y z) => true if odd number of truthy values, else false
pub fn prim_xor(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(false));
    }

    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    (SIG_OK, Value::bool(truthy_count % 2 == 1))
}
