use crate::arithmetic;
use crate::value::Value;

/// Variadic addition: (+ 1 2 3) -> 6, (+) -> 0
pub fn prim_add(args: &[Value]) -> Result<Value, String> {
    // Check that all args are numbers first
    for arg in args {
        match arg {
            Value::Int(_) | Value::Float(_) => {}
            _ => return Err("Type error: + requires numbers".to_string()),
        }
    }

    if args.is_empty() {
        return Ok(Value::Int(0)); // Identity element for addition
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::add_values(&result, arg)?;
    }
    Ok(result)
}

/// Variadic subtraction: (- 10 3 2) -> 5, (- 5) -> -5
pub fn prim_sub(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("- requires at least 1 argument".to_string());
    }

    if args.len() == 1 {
        return arithmetic::negate_value(&args[0]);
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::sub_values(&result, arg)?;
    }
    Ok(result)
}

/// Variadic multiplication: (* 2 3 4) -> 24, (*) -> 1
pub fn prim_mul(args: &[Value]) -> Result<Value, String> {
    // Check that all args are numbers first
    for arg in args {
        match arg {
            Value::Int(_) | Value::Float(_) => {}
            _ => return Err("Type error: * requires numbers".to_string()),
        }
    }

    if args.is_empty() {
        return Ok(Value::Int(1)); // Identity element for multiplication
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::mul_values(&result, arg)?;
    }
    Ok(result)
}

/// Variadic division: (/ 24 2 3) -> 4, (/ 5) -> 1/5
pub fn prim_div(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("/ requires at least 1 argument".to_string());
    }

    if args.len() == 1 {
        return arithmetic::reciprocal_value(&args[0]);
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = arithmetic::div_values(&result, arg)?;
    }
    Ok(result)
}

pub fn prim_mod(args: &[Value]) -> Result<Value, String> {
    // Euclidean modulo: result always has same sign as divisor (b)
    // Example: (mod -17 5) => 3 (because -17 = -4*5 + 3)
    if args.len() != 2 {
        return Err("mod requires exactly 2 arguments".to_string());
    }
    arithmetic::mod_values(&args[0], &args[1])
}

pub fn prim_rem(args: &[Value]) -> Result<Value, String> {
    // Truncated division remainder: result has same sign as dividend (a)
    // Example: (rem -17 5) => -2 (because -17 = -3*5 + -2)
    if args.len() != 2 {
        return Err("rem requires exactly 2 arguments".to_string());
    }
    arithmetic::remainder_values(&args[0], &args[1])
}

pub fn prim_abs(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("abs requires exactly 1 argument".to_string());
    }
    arithmetic::abs_value(&args[0])
}

pub fn prim_min(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("min requires at least 1 argument".to_string());
    }

    let mut min = args[0].clone();
    for arg in &args[1..] {
        // Check if arg is a number
        match arg {
            Value::Int(_) | Value::Float(_) => {
                min = arithmetic::min_values(&min, arg);
            }
            _ => return Err("Type error: min requires numbers".to_string()),
        }
    }
    Ok(min)
}

pub fn prim_max(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("max requires at least 1 argument".to_string());
    }

    let mut max = args[0].clone();
    for arg in &args[1..] {
        // Check if arg is a number
        match arg {
            Value::Int(_) | Value::Float(_) => {
                max = arithmetic::max_values(&max, arg);
            }
            _ => return Err("Type error: max requires numbers".to_string()),
        }
    }
    Ok(max)
}

pub fn prim_even(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("even? requires exactly 1 argument".to_string());
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 == 0)),
        _ => Err("Type error: even? requires an integer".to_string()),
    }
}

pub fn prim_odd(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("odd? requires exactly 1 argument".to_string());
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 != 0)),
        _ => Err("Type error: odd? requires an integer".to_string()),
    }
}
