use crate::arithmetic;
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Variadic addition: (+ 1 2 3) -> 6, (+) -> 0
pub fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    // Check that all args are numbers first
    for arg in args {
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("+: expected number, got {}", arg.type_name()),
                ),
            );
        }
    }

    if args.is_empty() {
        return (SIG_OK, Value::int(0)); // Identity element for addition
    }

    let mut result = args[0];
    for arg in &args[1..] {
        match arithmetic::add_values(&result, arg) {
            Ok(val) => result = val,
            Err(e) => return (SIG_ERROR, error_val("error", e)),
        }
    }
    (SIG_OK, result)
}

/// Variadic subtraction: (- 10 3 2) -> 5, (- 5) -> -5
pub fn prim_sub(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "-: expected at least 1 argument, got 0"),
        );
    }

    if args.len() == 1 {
        return match arithmetic::negate_value(&args[0]) {
            Ok(val) => (SIG_OK, val),
            Err(e) => (SIG_ERROR, error_val("error", e)),
        };
    }

    let mut result = args[0];
    for arg in &args[1..] {
        match arithmetic::sub_values(&result, arg) {
            Ok(val) => result = val,
            Err(e) => return (SIG_ERROR, error_val("error", e)),
        }
    }
    (SIG_OK, result)
}

/// Variadic multiplication: (* 2 3 4) -> 24, (*) -> 1
pub fn prim_mul(args: &[Value]) -> (SignalBits, Value) {
    // Check that all args are numbers first
    for arg in args {
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("*: expected number, got {}", arg.type_name()),
                ),
            );
        }
    }

    if args.is_empty() {
        return (SIG_OK, Value::int(1)); // Identity element for multiplication
    }

    let mut result = args[0];
    for arg in &args[1..] {
        match arithmetic::mul_values(&result, arg) {
            Ok(val) => result = val,
            Err(e) => return (SIG_ERROR, error_val("error", e)),
        }
    }
    (SIG_OK, result)
}

pub fn prim_mod(args: &[Value]) -> (SignalBits, Value) {
    // Euclidean modulo: result always has same sign as divisor (b)
    // Example: (mod -17 5) => 3 (because -17 = -4*5 + 3)
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mod: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    match arithmetic::mod_values(&args[0], &args[1]) {
        Ok(val) => (SIG_OK, val),
        Err(e) => (SIG_ERROR, error_val("error", e)),
    }
}

pub fn prim_rem(args: &[Value]) -> (SignalBits, Value) {
    // Truncated division remainder: result has same sign as dividend (a)
    // Example: (rem -17 5) => -2 (because -17 = -3*5 + -2)
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rem: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    match arithmetic::remainder_values(&args[0], &args[1]) {
        Ok(val) => (SIG_OK, val),
        Err(e) => (SIG_ERROR, error_val("error", e)),
    }
}

pub fn prim_abs(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("abs: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match arithmetic::abs_value(&args[0]) {
        Ok(val) => (SIG_OK, val),
        Err(e) => (SIG_ERROR, error_val("error", e)),
    }
}

pub fn prim_min(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "min: expected at least 1 argument, got 0"),
        );
    }

    let mut min = args[0];
    for arg in &args[1..] {
        // Check if arg is a number
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("min: expected number, got {}", arg.type_name()),
                ),
            );
        }
        min = arithmetic::min_values(&min, arg);
    }
    (SIG_OK, min)
}

pub fn prim_max(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "max: expected at least 1 argument, got 0"),
        );
    }

    let mut max = args[0];
    for arg in &args[1..] {
        // Check if arg is a number
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("max: expected number, got {}", arg.type_name()),
                ),
            );
        }
        max = arithmetic::max_values(&max, arg);
    }
    (SIG_OK, max)
}

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

pub fn prim_div_vm(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "/: expected at least 1 argument, got 0"),
        );
    }

    if args.len() == 1 {
        return match arithmetic::reciprocal_value(&args[0]) {
            Ok(val) => (SIG_OK, val),
            Err(msg) => (SIG_ERROR, error_val("type-error", msg)),
        };
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
            // Create a division-by-zero error
            return (SIG_ERROR, error_val("division-by-zero", "division by zero"));
        }

        match arithmetic::div_values(&result, arg) {
            Ok(val) => result = val,
            Err(msg) => {
                return (SIG_ERROR, error_val("type-error", msg));
            }
        }
    }
    (SIG_OK, result)
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "+",
        func: prim_add,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Sum all arguments. Returns 0 for no arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(+) ;=> 0\n(+ 1 2 3) ;=> 6",
        aliases: &[],
    },
    PrimitiveDef {
        name: "-",
        func: prim_sub,
        effect: Effect::none(),
        arity: Arity::AtLeast(1),
        doc: "Subtract arguments left-to-right. Single arg negates.",
        params: &["x", "ys"],
        category: "arithmetic",
        example: "(- 10 3 2) ;=> 5\n(- 5) ;=> -5",
        aliases: &[],
    },
    PrimitiveDef {
        name: "*",
        func: prim_mul,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Multiply all arguments. Returns 1 for no arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(*) ;=> 1\n(* 2 3 4) ;=> 24",
        aliases: &[],
    },
    PrimitiveDef {
        name: "/",
        func: prim_div_vm,
        effect: Effect::none(),
        arity: Arity::AtLeast(1),
        doc: "Divide arguments left-to-right. Single arg takes reciprocal.",
        params: &["x", "ys"],
        category: "arithmetic",
        example: "(/ 10 2) ;=> 5\n(/ 2) ;=> 0.5",
        aliases: &[],
    },
    PrimitiveDef {
        name: "mod",
        func: prim_mod,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Euclidean modulo. Result has same sign as divisor.",
        params: &["a", "b"],
        category: "arithmetic",
        example: "(mod 17 5) ;=> 2\n(mod -17 5) ;=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "%",
        func: prim_rem,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Truncated remainder. Result has same sign as dividend.",
        params: &["a", "b"],
        category: "arithmetic",
        example: "(% 17 5) ;=> 2\n(% -17 5) ;=> -2",
        aliases: &[],
    },
    PrimitiveDef {
        name: "rem",
        func: prim_rem,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Truncated remainder. Result has same sign as dividend.",
        params: &["a", "b"],
        category: "arithmetic",
        example: "(rem 17 5) ;=> 2\n(rem -17 5) ;=> -2",
        aliases: &[],
    },
    PrimitiveDef {
        name: "abs",
        func: prim_abs,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Absolute value.",
        params: &["x"],
        category: "arithmetic",
        example: "(abs -5) ;=> 5\n(abs 3) ;=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "min",
        func: prim_min,
        effect: Effect::none(),
        arity: Arity::AtLeast(1),
        doc: "Minimum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(min 3 1 4) ;=> 1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "max",
        func: prim_max,
        effect: Effect::none(),
        arity: Arity::AtLeast(1),
        doc: "Maximum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(max 3 1 4) ;=> 4",
        aliases: &[],
    },
    PrimitiveDef {
        name: "even?",
        func: prim_even,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Test if integer is even.",
        params: &["n"],
        category: "arithmetic",
        example: "(even? 4) ;=> #t\n(even? 3) ;=> #f",
        aliases: &[],
    },
    PrimitiveDef {
        name: "odd?",
        func: prim_odd,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Test if integer is odd.",
        params: &["n"],
        category: "arithmetic",
        example: "(odd? 3) ;=> #t\n(odd? 4) ;=> #f",
        aliases: &[],
    },
];
