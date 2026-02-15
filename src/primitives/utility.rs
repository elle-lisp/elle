//! Utility primitives (mod, remainder, even?, odd?)
use crate::error::LResult;
use crate::value::Value;

/// Modulo operation (result has same sign as divisor)
pub fn prim_mod(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err("mod requires exactly 2 arguments".to_string().into());
    }

    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                return Err("Division by zero".to_string().into());
            }
            // Lisp mod: result has same sign as divisor
            let rem = a % b;
            if rem == 0 {
                Ok(Value::Int(0))
            } else if (rem > 0) != (*b > 0) {
                Ok(Value::Int(rem + b))
            } else {
                Ok(Value::Int(rem))
            }
        }
        _ => Err("mod requires integers".to_string().into()),
    }
}

/// Remainder operation (result has same sign as dividend)
pub fn prim_remainder(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err("remainder requires exactly 2 arguments".to_string().into());
    }

    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                return Err("Division by zero".to_string().into());
            }
            let rem = a % b;
            // Adjust remainder to have same sign as dividend
            if (rem > 0 && *b < 0) || (rem < 0 && *b > 0) {
                Ok(Value::Int(rem + b))
            } else {
                Ok(Value::Int(rem))
            }
        }
        _ => Err("remainder requires integers".to_string().into()),
    }
}

/// Check if number is even
pub fn prim_even(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("even? requires exactly 1 argument".to_string().into());
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 == 0)),
        _ => Err("even? requires an integer".to_string().into()),
    }
}

/// Check if number is odd
pub fn prim_odd(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("odd? requires exactly 1 argument".to_string().into());
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Bool(n % 2 != 0)),
        _ => Err("odd? requires an integer".to_string().into()),
    }
}
