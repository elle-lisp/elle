//! Utility primitives (mod, remainder, even?, odd?)
use crate::value::{Condition, Value};

/// Modulo operation (result has same sign as divisor)
pub fn prim_mod(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "mod: expected 2 arguments, got {}",
            args.len()
        )));
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b == 0 {
                return Err(Condition::division_by_zero("mod: division by zero"));
            }
            // Lisp mod: result has same sign as divisor
            let rem = a % b;
            if rem == 0 {
                Ok(Value::int(0))
            } else if (rem > 0) != (b > 0) {
                Ok(Value::int(rem + b))
            } else {
                Ok(Value::int(rem))
            }
        }
        _ => Err(Condition::type_error(format!(
            "mod: expected integer, got {}",
            args[0].type_name()
        ))),
    }
}

/// Remainder operation (result has same sign as dividend)
pub fn prim_remainder(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "remainder: expected 2 arguments, got {}",
            args.len()
        )));
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b == 0 {
                return Err(Condition::division_by_zero("remainder: division by zero"));
            }
            let rem = a % b;
            // Adjust remainder to have same sign as dividend
            if (rem > 0 && b < 0) || (rem < 0 && b > 0) {
                Ok(Value::int(rem + b))
            } else {
                Ok(Value::int(rem))
            }
        }
        _ => Err(Condition::type_error(format!(
            "remainder: expected integer, got {}",
            args[0].type_name()
        ))),
    }
}

/// Check if number is even
pub fn prim_even(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "even?: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::bool(n % 2 == 0)),
        _ => Err(Condition::type_error(format!(
            "even?: expected integer, got {}",
            args[0].type_name()
        ))),
    }
}

/// Check if number is odd
pub fn prim_odd(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "odd?: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::bool(n % 2 != 0)),
        _ => Err(Condition::type_error(format!(
            "odd?: expected integer, got {}",
            args[0].type_name()
        ))),
    }
}
