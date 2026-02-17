use crate::value::{Condition, Value};
use std::f64::consts::{E, PI};

pub fn prim_sqrt(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "sqrt: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::float((n as f64).sqrt())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f.sqrt())),
            None => Err(Condition::type_error(format!(
                "sqrt: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_sin(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "sin: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::float((n as f64).sin())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f.sin())),
            None => Err(Condition::type_error(format!(
                "sin: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_cos(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "cos: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::float((n as f64).cos())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f.cos())),
            None => Err(Condition::type_error(format!(
                "cos: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_tan(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "tan: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::float((n as f64).tan())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f.tan())),
            None => Err(Condition::type_error(format!(
                "tan: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_log(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() || args.len() > 2 {
        return Err(Condition::arity_error(format!(
            "log: expected 1-2 arguments, got {}",
            args.len()
        )));
    }

    let value = match args[0].as_int() {
        Some(n) => n as f64,
        None => match args[0].as_float() {
            Some(f) => f,
            None => {
                return Err(Condition::type_error(format!(
                    "log: expected number, got {}",
                    args[0].type_name()
                )))
            }
        },
    };

    if args.len() == 1 {
        // Natural logarithm
        Ok(Value::float(value.ln()))
    } else {
        // Logarithm with specified base
        let base = match args[1].as_int() {
            Some(n) => n as f64,
            None => match args[1].as_float() {
                Some(f) => f,
                None => {
                    return Err(Condition::type_error(format!(
                        "log: expected number, got {}",
                        args[1].type_name()
                    )))
                }
            },
        };
        Ok(Value::float(value.log(base)))
    }
}

pub fn prim_exp(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "exp: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::float((n as f64).exp())),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::float(f.exp())),
            None => Err(Condition::type_error(format!(
                "exp: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_pow(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "pow: expected 2 arguments, got {}",
            args.len()
        )));
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b < 0 {
                Ok(Value::float((a as f64).powf(b as f64)))
            } else {
                Ok(Value::int(a.pow(b as u32)))
            }
        }
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => Ok(Value::float(a.powf(b))),
            _ => Err(Condition::type_error(format!(
                "pow: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_floor(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "floor: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::int(f.floor() as i64)),
            None => Err(Condition::type_error(format!(
                "floor: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_ceil(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "ceil: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::int(f.ceil() as i64)),
            None => Err(Condition::type_error(format!(
                "ceil: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_round(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "round: expected 1 argument, got {}",
            args.len()
        )));
    }

    match args[0].as_int() {
        Some(n) => Ok(Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => Ok(Value::int(f.round() as i64)),
            None => Err(Condition::type_error(format!(
                "round: expected number, got {}",
                args[0].type_name()
            ))),
        },
    }
}

pub fn prim_pi(_args: &[Value]) -> Result<Value, Condition> {
    Ok(Value::float(PI))
}

pub fn prim_e(_args: &[Value]) -> Result<Value, Condition> {
    Ok(Value::float(E))
}
