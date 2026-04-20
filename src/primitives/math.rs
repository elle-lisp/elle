use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::f64::consts::{E, PI};

// ---------------------------------------------------------------------------
// Helpers — eliminate the copy-paste
// ---------------------------------------------------------------------------

/// Unary op: number → float (e.g. sqrt, sin, cos, …)
fn unary_float(name: &str, args: &[Value], op: fn(f64) -> f64) -> (SignalBits, Value) {
    match args[0].as_number() {
        Some(n) => (SIG_OK, Value::float(op(n))),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{name}: expected number, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Unary op: number → int (floor, ceil, round — ints pass through)
fn unary_to_int(name: &str, args: &[Value], op: fn(f64) -> f64) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::int(n));
    }
    match args[0].as_float() {
        Some(f) => (SIG_OK, Value::int(op(f) as i64)),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{name}: expected number, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Extract a single numeric arg as f64, or return a type error.
fn require_number(name: &str, v: &Value) -> Result<f64, (SignalBits, Value)> {
    v.as_number().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{name}: expected number, got {}", v.type_name()),
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Unary float ops — each is a one-liner via the helper
// ---------------------------------------------------------------------------

fn prim_sqrt(args: &[Value]) -> (SignalBits, Value) {
    unary_float("sqrt", args, f64::sqrt)
}
fn prim_sin(args: &[Value]) -> (SignalBits, Value) {
    unary_float("sin", args, f64::sin)
}
fn prim_cos(args: &[Value]) -> (SignalBits, Value) {
    unary_float("cos", args, f64::cos)
}
fn prim_tan(args: &[Value]) -> (SignalBits, Value) {
    unary_float("tan", args, f64::tan)
}
fn prim_exp(args: &[Value]) -> (SignalBits, Value) {
    unary_float("exp", args, f64::exp)
}
fn prim_asin(args: &[Value]) -> (SignalBits, Value) {
    unary_float("asin", args, f64::asin)
}
fn prim_acos(args: &[Value]) -> (SignalBits, Value) {
    unary_float("acos", args, f64::acos)
}
fn prim_atan(args: &[Value]) -> (SignalBits, Value) {
    unary_float("atan", args, f64::atan)
}
fn prim_sinh(args: &[Value]) -> (SignalBits, Value) {
    unary_float("sinh", args, f64::sinh)
}
fn prim_cosh(args: &[Value]) -> (SignalBits, Value) {
    unary_float("cosh", args, f64::cosh)
}
fn prim_tanh(args: &[Value]) -> (SignalBits, Value) {
    unary_float("tanh", args, f64::tanh)
}
fn prim_log2(args: &[Value]) -> (SignalBits, Value) {
    unary_float("log2", args, f64::log2)
}
fn prim_log10(args: &[Value]) -> (SignalBits, Value) {
    unary_float("log10", args, f64::log10)
}
fn prim_trunc(args: &[Value]) -> (SignalBits, Value) {
    unary_float("trunc", args, f64::trunc)
}
fn prim_cbrt(args: &[Value]) -> (SignalBits, Value) {
    unary_float("cbrt", args, f64::cbrt)
}
fn prim_exp2(args: &[Value]) -> (SignalBits, Value) {
    unary_float("exp2", args, f64::exp2)
}

// ---------------------------------------------------------------------------
// Unary int-returning ops (int passthrough, float → int)
// ---------------------------------------------------------------------------

fn prim_floor(args: &[Value]) -> (SignalBits, Value) {
    unary_to_int("floor", args, f64::floor)
}
fn prim_ceil(args: &[Value]) -> (SignalBits, Value) {
    unary_to_int("ceil", args, f64::ceil)
}
fn prim_round(args: &[Value]) -> (SignalBits, Value) {
    unary_to_int("round", args, f64::round)
}

// ---------------------------------------------------------------------------
// Special cases — log, pow, atan2 have non-trivial signatures
// ---------------------------------------------------------------------------

fn prim_log(args: &[Value]) -> (SignalBits, Value) {
    let value = match require_number("log", &args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if args.len() == 1 {
        (SIG_OK, Value::float(value.ln()))
    } else {
        let base = match require_number("log", &args[1]) {
            Ok(v) => v,
            Err(e) => return e,
        };
        (SIG_OK, Value::float(value.log(base)))
    }
}

fn prim_pow(args: &[Value]) -> (SignalBits, Value) {
    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int()) {
        if b < 0 {
            (SIG_OK, Value::float((a as f64).powf(b as f64)))
        } else {
            (SIG_OK, Value::int(a.pow(b as u32)))
        }
    } else {
        match (args[0].as_number(), args[1].as_number()) {
            (Some(a), Some(b)) => (SIG_OK, Value::float(a.powf(b))),
            _ => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("pow: expected number, got {}", args[0].type_name()),
                ),
            ),
        }
    }
}

fn prim_fmod(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_number("fmod", &args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = match require_number("fmod", &args[1]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if b == 0.0 {
        return (
            SIG_ERROR,
            error_val("division-by-zero", "fmod: division by zero"),
        );
    }
    (SIG_OK, Value::float(a % b))
}

fn prim_atan2(args: &[Value]) -> (SignalBits, Value) {
    let y = match require_number("atan2", &args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let x = match require_number("atan2", &args[1]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    (SIG_OK, Value::float(y.atan2(x)))
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

fn prim_pi(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(PI))
}
fn prim_e(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(E))
}
fn prim_inf(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::INFINITY))
}
fn prim_neg_inf(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NEG_INFINITY))
}
fn prim_nan(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NAN))
}

// ---------------------------------------------------------------------------
// IEEE 754 bitcast
// ---------------------------------------------------------------------------

fn prim_f32_bits(args: &[Value]) -> (SignalBits, Value) {
    match require_number("math/f32-bits", &args[0]) {
        Ok(f) => (SIG_OK, Value::int((f as f32).to_bits() as i64)),
        Err(e) => e,
    }
}

fn prim_f32_from_bits(args: &[Value]) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(i) => (SIG_OK, Value::float(f32::from_bits(i as u32) as f64)),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "math/f32-from-bits: expected int, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "math/sqrt", func: prim_sqrt, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the square root of a number.",
        params: &["x"], category: "math", example: "(math/sqrt 16)",
        aliases: &["sqrt"],
    },
    PrimitiveDef {
        name: "math/sin", func: prim_sin, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the sine of a number (in radians).",
        params: &["x"], category: "math", example: "(math/sin 0)",
        aliases: &["sin"],
    },
    PrimitiveDef {
        name: "math/cos", func: prim_cos, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the cosine of a number (in radians).",
        params: &["x"], category: "math", example: "(math/cos 0)",
        aliases: &["cos"],
    },
    PrimitiveDef {
        name: "math/tan", func: prim_tan, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the tangent of a number (in radians).",
        params: &["x"], category: "math", example: "(math/tan 0)",
        aliases: &["tan"],
    },
    PrimitiveDef {
        name: "math/log", func: prim_log, signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Returns the natural logarithm of x, or logarithm with specified base.",
        params: &["x", "base"], category: "math", example: "(math/log 2.718281828)",
        aliases: &["log"],
    },
    PrimitiveDef {
        name: "math/exp", func: prim_exp, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns e raised to the power of x.",
        params: &["x"], category: "math", example: "(math/exp 1)",
        aliases: &["exp"],
    },
    PrimitiveDef {
        name: "math/pow", func: prim_pow, signal: Signal::errors(),
        arity: Arity::Exact(2), doc: "Returns x raised to the power of y.",
        params: &["x", "y"], category: "math", example: "(math/pow 2 8)",
        aliases: &["pow"],
    },
    PrimitiveDef {
        name: "math/floor", func: prim_floor, signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the largest integer less than or equal to x.",
        params: &["x"], category: "math", example: "(math/floor 3.7)",
        aliases: &["floor"],
    },
    PrimitiveDef {
        name: "math/ceil", func: prim_ceil, signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the smallest integer greater than or equal to x.",
        params: &["x"], category: "math", example: "(math/ceil 3.2)",
        aliases: &["ceil"],
    },
    PrimitiveDef {
        name: "math/round", func: prim_round, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the nearest integer to x.",
        params: &["x"], category: "math", example: "(math/round 3.5)",
        aliases: &["round"],
    },
    PrimitiveDef {
        name: "math/pi", func: prim_pi, signal: Signal::silent(),
        arity: Arity::Exact(0), doc: "The mathematical constant pi (π).",
        params: &[], category: "math", example: "(math/pi)",
        aliases: &["pi"],
    },
    PrimitiveDef {
        name: "math/e", func: prim_e, signal: Signal::silent(),
        arity: Arity::Exact(0), doc: "The mathematical constant e (Euler's number).",
        params: &[], category: "math", example: "(math/e)",
        aliases: &["e"],
    },
    PrimitiveDef {
        name: "math/asin", func: prim_asin, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the arcsine of a number (in radians).",
        params: &["x"], category: "math", example: "(math/asin 1)",
        aliases: &["asin"],
    },
    PrimitiveDef {
        name: "math/acos", func: prim_acos, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the arccosine of a number (in radians).",
        params: &["x"], category: "math", example: "(math/acos 1)",
        aliases: &["acos"],
    },
    PrimitiveDef {
        name: "math/atan", func: prim_atan, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the arctangent of a number (in radians).",
        params: &["x"], category: "math", example: "(math/atan 1)",
        aliases: &["atan"],
    },
    PrimitiveDef {
        name: "math/fmod", func: prim_fmod, signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Floating-point remainder. Returns a - floor(a/b) * b.",
        params: &["a", "b"], category: "math", example: "(math/fmod 5.5 2.0) #=> 1.5",
        aliases: &["fmod"],
    },
    PrimitiveDef {
        name: "math/atan2", func: prim_atan2, signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Returns the arctangent of y/x (in radians), using the signs of both arguments to determine the quadrant.",
        params: &["y", "x"], category: "math", example: "(math/atan2 1 1)",
        aliases: &["atan2"],
    },
    PrimitiveDef {
        name: "math/sinh", func: prim_sinh, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the hyperbolic sine of a number.",
        params: &["x"], category: "math", example: "(math/sinh 1)",
        aliases: &["sinh"],
    },
    PrimitiveDef {
        name: "math/cosh", func: prim_cosh, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the hyperbolic cosine of a number.",
        params: &["x"], category: "math", example: "(math/cosh 1)",
        aliases: &["cosh"],
    },
    PrimitiveDef {
        name: "math/tanh", func: prim_tanh, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the hyperbolic tangent of a number.",
        params: &["x"], category: "math", example: "(math/tanh 1)",
        aliases: &["tanh"],
    },
    PrimitiveDef {
        name: "math/log2", func: prim_log2, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the base-2 logarithm of a number.",
        params: &["x"], category: "math", example: "(math/log2 8)",
        aliases: &["log2"],
    },
    PrimitiveDef {
        name: "math/log10", func: prim_log10, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the base-10 logarithm of a number.",
        params: &["x"], category: "math", example: "(math/log10 100)",
        aliases: &["log10"],
    },
    PrimitiveDef {
        name: "math/trunc", func: prim_trunc, signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the integer part of a number, truncating toward zero.",
        params: &["x"], category: "math", example: "(math/trunc 3.7)",
        aliases: &["trunc"],
    },
    PrimitiveDef {
        name: "math/cbrt", func: prim_cbrt, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns the cube root of a number.",
        params: &["x"], category: "math", example: "(math/cbrt 27)",
        aliases: &["cbrt"],
    },
    PrimitiveDef {
        name: "math/exp2", func: prim_exp2, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Returns 2 raised to the power of x.",
        params: &["x"], category: "math", example: "(math/exp2 3)",
        aliases: &["exp2"],
    },
    PrimitiveDef {
        name: "math/inf", func: prim_inf, signal: Signal::silent(),
        arity: Arity::Exact(0), doc: "Positive infinity (IEEE 754).",
        params: &[], category: "math", example: "(math/inf)",
        aliases: &["+inf", "inf"],
    },
    PrimitiveDef {
        name: "math/-inf", func: prim_neg_inf, signal: Signal::silent(),
        arity: Arity::Exact(0), doc: "Negative infinity (IEEE 754).",
        params: &[], category: "math", example: "(math/-inf)",
        aliases: &["-inf"],
    },
    PrimitiveDef {
        name: "math/nan", func: prim_nan, signal: Signal::silent(),
        arity: Arity::Exact(0), doc: "Not-a-number (IEEE 754 NaN).",
        params: &[], category: "math", example: "(math/nan)",
        aliases: &["nan"],
    },
    PrimitiveDef {
        name: "math/f32-bits", func: prim_f32_bits, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Return the IEEE 754 f32 bit pattern of a number as an integer.",
        params: &["x"], category: "math", example: "(math/f32-bits 1.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "math/f32-from-bits", func: prim_f32_from_bits, signal: Signal::errors(),
        arity: Arity::Exact(1), doc: "Reinterpret an integer as an IEEE 754 f32 bit pattern.",
        params: &["bits"], category: "math", example: "(math/f32-from-bits 1065353216)",
        aliases: &[],
    },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unary_float_ops_with_int() {
        let args = [Value::int(16)];
        let (sig, val) = prim_sqrt(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_float(), Some(4.0));
    }

    #[test]
    fn unary_float_ops_with_float() {
        let args = [Value::float(16.0)];
        let (sig, val) = prim_sqrt(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_float(), Some(4.0));
    }

    #[test]
    fn unary_float_ops_type_error() {
        let args = [Value::string("hello")];
        let (sig, _) = prim_sqrt(&args);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn floor_passthrough_int() {
        let args = [Value::int(5)];
        let (sig, val) = prim_floor(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(5));
    }

    #[test]
    fn floor_truncates_float() {
        let args = [Value::float(3.7)];
        let (sig, val) = prim_floor(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(3));
    }

    #[test]
    fn ceil_rounds_up() {
        let args = [Value::float(3.2)];
        let (sig, val) = prim_ceil(&args);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(4));
    }

    #[test]
    fn round_rounds() {
        let (_, v1) = prim_round(&[Value::float(3.5)]);
        assert_eq!(v1.as_int(), Some(4));
        let (_, v2) = prim_round(&[Value::float(3.4)]);
        assert_eq!(v2.as_int(), Some(3));
    }

    #[test]
    fn log_natural() {
        let (sig, val) = prim_log(&[Value::float(E)]);
        assert_eq!(sig, SIG_OK);
        assert!((val.as_float().unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn log_with_base() {
        let (sig, val) = prim_log(&[Value::int(8), Value::int(2)]);
        assert_eq!(sig, SIG_OK);
        assert!((val.as_float().unwrap() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn pow_int_positive() {
        let (sig, val) = prim_pow(&[Value::int(2), Value::int(8)]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(256));
    }

    #[test]
    fn pow_int_negative_exponent() {
        let (sig, val) = prim_pow(&[Value::int(2), Value::int(-1)]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_float(), Some(0.5));
    }

    #[test]
    fn pow_float() {
        let (sig, val) = prim_pow(&[Value::float(4.0), Value::float(0.5)]);
        assert_eq!(sig, SIG_OK);
        assert!((val.as_float().unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn atan2_basic() {
        let (sig, val) = prim_atan2(&[Value::int(1), Value::int(1)]);
        assert_eq!(sig, SIG_OK);
        assert!((val.as_float().unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-10);
    }

    #[test]
    fn trig_round_trip() {
        // sin(0) == 0, cos(0) == 1
        let (_, s) = prim_sin(&[Value::int(0)]);
        assert_eq!(s.as_float(), Some(0.0));
        let (_, c) = prim_cos(&[Value::int(0)]);
        assert_eq!(c.as_float(), Some(1.0));
    }

    #[test]
    fn constants() {
        let (_, p) = prim_pi(&[]);
        assert_eq!(p.as_float(), Some(PI));
        let (_, e) = prim_e(&[]);
        assert_eq!(e.as_float(), Some(E));
        let (_, inf) = prim_inf(&[]);
        assert_eq!(inf.as_float(), Some(f64::INFINITY));
        let (_, ninf) = prim_neg_inf(&[]);
        assert_eq!(ninf.as_float(), Some(f64::NEG_INFINITY));
        let (_, nan) = prim_nan(&[]);
        assert!(nan.as_float().unwrap().is_nan());
    }
}
