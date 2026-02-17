use crate::arithmetic;
use crate::error::LResult;
use crate::value::{Condition, Value};
use crate::vm::core::VM;

/// Variadic addition: (+ 1 2 3) -> 6, (+) -> 0
pub fn prim_add(args: &[Value]) -> Result<Value, Condition> {
    // Check that all args are numbers first
    for arg in args {
        if !arg.is_number() {
            return Err(Condition::type_error(format!(
                "+: expected number, got {}",
                arg.type_name()
            )));
        }
    }

    if args.is_empty() {
        return Ok(Value::int(0)); // Identity element for addition
    }

    let mut result = args[0];
    for arg in &args[1..] {
        result = arithmetic::add_values(&result, arg).map_err(Condition::error)?;
    }
    Ok(result)
}

/// Variadic subtraction: (- 10 3 2) -> 5, (- 5) -> -5
pub fn prim_sub(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "-: expected at least 1 argument, got 0".to_string(),
        ));
    }

    if args.len() == 1 {
        return arithmetic::negate_value(&args[0]).map_err(Condition::error);
    }

    let mut result = args[0];
    for arg in &args[1..] {
        result = arithmetic::sub_values(&result, arg).map_err(Condition::error)?;
    }
    Ok(result)
}

/// Variadic multiplication: (* 2 3 4) -> 24, (*) -> 1
pub fn prim_mul(args: &[Value]) -> Result<Value, Condition> {
    // Check that all args are numbers first
    for arg in args {
        if !arg.is_number() {
            return Err(Condition::type_error(format!(
                "*: expected number, got {}",
                arg.type_name()
            )));
        }
    }

    if args.is_empty() {
        return Ok(Value::int(1)); // Identity element for multiplication
    }

    let mut result = args[0];
    for arg in &args[1..] {
        result = arithmetic::mul_values(&result, arg).map_err(Condition::error)?;
    }
    Ok(result)
}

/// Variadic division: (/ 24 2 3) -> 4, (/ 5) -> 1/5
pub fn prim_div(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "/: expected at least 1 argument, got 0".to_string(),
        ));
    }

    if args.len() == 1 {
        return arithmetic::reciprocal_value(&args[0]).map_err(Condition::error);
    }

    let mut result = args[0];
    for arg in &args[1..] {
        result = arithmetic::div_values(&result, arg).map_err(Condition::error)?;
    }
    Ok(result)
}

pub fn prim_mod(args: &[Value]) -> Result<Value, Condition> {
    // Euclidean modulo: result always has same sign as divisor (b)
    // Example: (mod -17 5) => 3 (because -17 = -4*5 + 3)
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "mod: expected 2 arguments, got {}",
            args.len()
        )));
    }
    arithmetic::mod_values(&args[0], &args[1]).map_err(Condition::error)
}

pub fn prim_rem(args: &[Value]) -> Result<Value, Condition> {
    // Truncated division remainder: result has same sign as dividend (a)
    // Example: (rem -17 5) => -2 (because -17 = -3*5 + -2)
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "rem: expected 2 arguments, got {}",
            args.len()
        )));
    }
    arithmetic::remainder_values(&args[0], &args[1]).map_err(Condition::error)
}

pub fn prim_abs(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "abs: expected 1 argument, got {}",
            args.len()
        )));
    }
    arithmetic::abs_value(&args[0]).map_err(Condition::error)
}

pub fn prim_min(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "min: expected at least 1 argument, got 0".to_string(),
        ));
    }

    let mut min = args[0];
    for arg in &args[1..] {
        // Check if arg is a number
        if !arg.is_number() {
            return Err(Condition::type_error(format!(
                "min: expected number, got {}",
                arg.type_name()
            )));
        }
        min = arithmetic::min_values(&min, arg);
    }
    Ok(min)
}

pub fn prim_max(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "max: expected at least 1 argument, got 0".to_string(),
        ));
    }

    let mut max = args[0];
    for arg in &args[1..] {
        // Check if arg is a number
        if !arg.is_number() {
            return Err(Condition::type_error(format!(
                "max: expected number, got {}",
                arg.type_name()
            )));
        }
        max = arithmetic::max_values(&max, arg);
    }
    Ok(max)
}

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

pub fn prim_div_vm(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.is_empty() {
        let cond = Condition::arity_error("/: expected at least 1 argument, got 0");
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    if args.len() == 1 {
        match arithmetic::reciprocal_value(&args[0]) {
            Ok(val) => return Ok(val),
            Err(msg) => {
                let cond = Condition::type_error(msg);
                vm.current_exception = Some(std::rc::Rc::new(cond));
                return Ok(Value::NIL);
            }
        }
    }

    let mut result = args[0];
    for arg in &args[1..] {
        // Check for division by zero
        let is_zero = match (result.as_int(), arg.as_int()) {
            (Some(_), Some(y)) => y == 0,
            _ => match (result.as_float(), arg.as_float()) {
                (Some(_), Some(y)) => y == 0.0,
                _ => match (result.as_int(), arg.as_float()) {
                    (Some(_), Some(y)) => y == 0.0,
                    _ => match (result.as_float(), arg.as_int()) {
                        (Some(_), Some(y)) => y == 0,
                        _ => false,
                    },
                },
            },
        };

        if is_zero {
            // Create a division-by-zero Condition
            let cond = crate::value::Condition::division_by_zero("division by zero")
                .with_field(0, result) // dividend
                .with_field(1, *arg); // divisor
            vm.current_exception = Some(std::rc::Rc::new(cond));
            return Ok(Value::NIL);
        }

        match arithmetic::div_values(&result, arg) {
            Ok(val) => result = val,
            Err(msg) => {
                let cond = Condition::type_error(msg);
                vm.current_exception = Some(std::rc::Rc::new(cond));
                return Ok(Value::NIL);
            }
        }
    }
    Ok(result)
}
