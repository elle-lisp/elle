use crate::value::{Condition, Value};

/// Logical NOT operation
pub fn prim_not(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "not: expected 1 argument, got {}",
            args.len()
        )));
    }
    Ok(Value::bool(!args[0].is_truthy()))
}

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub fn prim_and(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Ok(Value::bool(true));
    }

    for arg in &args[..args.len() - 1] {
        if !arg.is_truthy() {
            return Ok(*arg);
        }
    }

    Ok(args[args.len() - 1])
}

/// Logical OR operation
/// (or) => false
/// (or x) => x
/// (or x y z) => x if truthy, else next truthy or z
pub fn prim_or(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Ok(Value::bool(false));
    }

    for arg in &args[..args.len() - 1] {
        if arg.is_truthy() {
            return Ok(*arg);
        }
    }

    Ok(args[args.len() - 1])
}

/// Logical XOR operation
/// (xor) => false
/// (xor x) => x (as bool)
/// (xor x y z) => true if odd number of truthy values, else false
pub fn prim_xor(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Ok(Value::bool(false));
    }

    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    Ok(Value::bool(truthy_count % 2 == 1))
}
