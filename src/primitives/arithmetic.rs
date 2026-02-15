use crate::arithmetic;
use crate::error::{LError, LResult};
use crate::value::Value;

/// Variadic addition: (+ 1 2 3) -> 6, (+) -> 0
pub fn prim_add(args: &[Value]) -> LResult<Value> {
    // Check that all args are numbers first
    for arg in args {
        match arg {
            Value::Int(_) | Value::Float(_) => {}
            _ => return Err(LError::type_mismatch("number", arg.type_name())),
        }
    }

    if args.is_empty() {
        return Ok(Value::Int(0)); // Identity element for addition
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::add_values(&result, arg).map_err(LError::from)?;
    }
    Ok(result)
}

/// Variadic subtraction: (- 10 3 2) -> 5, (- 5) -> -5
pub fn prim_sub(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err(LError::arity_at_least(1, args.len()));
    }

    if args.len() == 1 {
        return arithmetic::negate_value(&args[0]).map_err(LError::from);
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::sub_values(&result, arg).map_err(LError::from)?;
    }
    Ok(result)
}

/// Variadic multiplication: (* 2 3 4) -> 24, (*) -> 1
pub fn prim_mul(args: &[Value]) -> LResult<Value> {
    // Check that all args are numbers first
    for arg in args {
        match arg {
            Value::Int(_) | Value::Float(_) => {}
            _ => return Err(LError::type_mismatch("number", arg.type_name())),
        }
    }

    if args.is_empty() {
        return Ok(Value::Int(1)); // Identity element for multiplication
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::mul_values(&result, arg).map_err(LError::from)?;
    }
    Ok(result)
}

/// Variadic division: (/ 24 2 3) -> 4, (/ 5) -> 1/5
pub fn prim_div(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err(LError::arity_at_least(1, args.len()));
    }

    if args.len() == 1 {
        return arithmetic::reciprocal_value(&args[0]).map_err(LError::from);
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::div_values(&result, arg).map_err(LError::from)?;
    }
    Ok(result)
}

pub fn prim_mod(args: &[Value]) -> LResult<Value> {
    // Euclidean modulo: result always has same sign as divisor (b)
    // Example: (mod -17 5) => 3 (because -17 = -4*5 + 3)
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }
    arithmetic::mod_values(&args[0], &args[1]).map_err(LError::from)
}

pub fn prim_rem(args: &[Value]) -> LResult<Value> {
    // Truncated division remainder: result has same sign as dividend (a)
    // Example: (rem -17 5) => -2 (because -17 = -3*5 + -2)
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }
    arithmetic::remainder_values(&args[0], &args[1]).map_err(LError::from)
}

pub fn prim_abs(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    arithmetic::abs_value(&args[0]).map_err(LError::from)
}

pub fn prim_min(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err(LError::arity_at_least(1, args.len()));
    }

    let mut min = args[0].clone();
    for arg in &args[1..] {
        // Check if arg is a number
        match arg {
            Value::Int(_) | Value::Float(_) => {
                min = arithmetic::min_values(&min, arg);
            }
            _ => return Err(LError::type_mismatch("number", arg.type_name())),
        }
    }
    Ok(min)
}

pub fn prim_max(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err(LError::arity_at_least(1, args.len()));
    }

    let mut max = args[0].clone();
    for arg in &args[1..] {
        // Check if arg is a number
        match arg {
            Value::Int(_) | Value::Float(_) => {
                max = arithmetic::max_values(&max, arg);
            }
            _ => return Err(LError::type_mismatch("number", arg.type_name())),
        }
    }
    Ok(max)
}

pub fn prim_even(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 == 0)),
        _ => Err(LError::type_mismatch("integer", args[0].type_name())),
    }
}

pub fn prim_odd(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 != 0)),
        _ => Err(LError::type_mismatch("integer", args[0].type_name())),
    }
}
