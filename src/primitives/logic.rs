use crate::value::Value;

/// Logical NOT operation
pub fn prim_not(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("not requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(!args[0].is_truthy()))
}

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub fn prim_and(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Bool(true));
    }

    for arg in &args[..args.len() - 1] {
        if !arg.is_truthy() {
            return Ok(arg.clone());
        }
    }

    Ok(args[args.len() - 1].clone())
}

/// Logical OR operation
/// (or) => false
/// (or x) => x
/// (or x y z) => x if truthy, else next truthy or z
pub fn prim_or(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }

    for arg in &args[..args.len() - 1] {
        if arg.is_truthy() {
            return Ok(arg.clone());
        }
    }

    Ok(args[args.len() - 1].clone())
}

/// Logical XOR operation
/// (xor) => false
/// (xor x) => x (as bool)
/// (xor x y z) => true if odd number of truthy values, else false
pub fn prim_xor(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }

    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    Ok(Value::Bool(truthy_count % 2 == 1))
}
