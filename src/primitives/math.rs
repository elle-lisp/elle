use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::f64::consts::{E, PI};

// ---------------------------------------------------------------------------
// Helpers — eliminate the copy-paste
// ---------------------------------------------------------------------------

/// Unary op: number → float (e.g. sqrt, sin, cos, …)
fn unary_float(
    name: &str,
    args: &[Value],
    op: fn(f64) -> f64,
    ctx: &mut NativeCtx,
) -> (SignalBits, Value) {
    match args[0].as_number() {
        Some(n) => (SIG_OK, Value::float(op(n))),
        None => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{name}: expected number, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Extract a single numeric arg as f64, or return a type error.
fn require_number(name: &str, v: &Value, ctx: &mut NativeCtx) -> Result<f64, (SignalBits, Value)> {
    v.as_number().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{name}: expected number, got {}", v.type_name()),
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Unary float ops — each is a one-liner via the helper
// ---------------------------------------------------------------------------

fn prim_sqrt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sqrt", args, f64::sqrt, ctx)
}
fn prim_sin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sin", args, f64::sin, ctx)
}
fn prim_cos(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cos", args, f64::cos, ctx)
}
fn prim_tan(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("tan", args, f64::tan, ctx)
}
fn prim_exp(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("exp", args, f64::exp, ctx)
}
fn prim_asin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("asin", args, f64::asin, ctx)
}
fn prim_acos(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("acos", args, f64::acos, ctx)
}
fn prim_atan(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("atan", args, f64::atan, ctx)
}
fn prim_sinh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sinh", args, f64::sinh, ctx)
}
fn prim_cosh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cosh", args, f64::cosh, ctx)
}
fn prim_tanh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("tanh", args, f64::tanh, ctx)
}
fn prim_log2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("log2", args, f64::log2, ctx)
}
fn prim_log10(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("log10", args, f64::log10, ctx)
}
fn prim_trunc(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("trunc", args, f64::trunc, ctx)
}
fn prim_cbrt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cbrt", args, f64::cbrt, ctx)
}
fn prim_exp2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("exp2", args, f64::exp2, ctx)
}

// ---------------------------------------------------------------------------
// Special cases — log, pow, atan2 have non-trivial signatures
// ---------------------------------------------------------------------------

fn prim_log(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = match require_number("log", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if args.len() == 1 {
        (SIG_OK, Value::float(value.ln()))
    } else {
        let base = match require_number("log", &args[1], ctx) {
            Ok(v) => v,
            Err(e) => return e,
        };
        (SIG_OK, Value::float(value.log(base)))
    }
}

fn prim_pow(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
                ctx.error(
                    "type-error",
                    format!("pow: expected number, got {}", args[0].type_name()),
                ),
            ),
        }
    }
}

fn prim_fmod(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = match require_number("fmod", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = match require_number("fmod", &args[1], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if b == 0.0 {
        return (
            SIG_ERROR,
            ctx.error("division-by-zero", "fmod: division by zero"),
        );
    }
    (SIG_OK, Value::float(a % b))
}

fn prim_atan2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let y = match require_number("atan2", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let x = match require_number("atan2", &args[1], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    (SIG_OK, Value::float(y.atan2(x)))
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

fn prim_pi(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(PI))
}
fn prim_e(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(E))
}
fn prim_inf(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::INFINITY))
}
fn prim_neg_inf(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NEG_INFINITY))
}
fn prim_nan(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NAN))
}

// ---------------------------------------------------------------------------
// IEEE 754 bitcast
// ---------------------------------------------------------------------------

fn prim_f32_bits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match require_number("math/f32-bits", &args[0], ctx) {
        Ok(f) => (SIG_OK, Value::int((f as f32).to_bits() as i64)),
        Err(e) => e,
    }
}

fn prim_f32_from_bits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(i) => (SIG_OK, Value::float(f32::from_bits(i as u32) as f64)),
        None => (
            SIG_ERROR,
            ctx.error(
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

primitive! {
    "math/sqrt" => prim_sqrt {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the square root of a number.",
        params: &["x"],
        category: "math",
        example: "(math/sqrt 16)",
        aliases: &["sqrt"],
        effect: RegionEffect::Immediate,
    }
    "math/sin" => prim_sin {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the sine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/sin 0)",
        aliases: &["sin"],
        effect: RegionEffect::Immediate,
    }
    "math/cos" => prim_cos {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the cosine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/cos 0)",
        aliases: &["cos"],
        effect: RegionEffect::Immediate,
    }
    "math/tan" => prim_tan {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the tangent of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/tan 0)",
        aliases: &["tan"],
        effect: RegionEffect::Immediate,
    }
    "math/log" => prim_log {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Returns the natural logarithm of x, or logarithm with specified base.",
        params: &["x", "base"],
        category: "math",
        example: "(math/log 2.718281828)",
        aliases: &["log"],
        effect: RegionEffect::Immediate,
    }
    "math/exp" => prim_exp {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns e raised to the power of x.",
        params: &["x"],
        category: "math",
        example: "(math/exp 1)",
        aliases: &["exp"],
        effect: RegionEffect::Immediate,
    }
    "math/pow" => prim_pow {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Returns x raised to the power of y.",
        params: &["x", "y"],
        category: "math",
        example: "(math/pow 2 8)",
        aliases: &["pow"],
        effect: RegionEffect::Immediate,
    }
    "math/pi" => prim_pi {
        doc: "The mathematical constant pi (π).",
        category: "math",
        example: "(math/pi)",
        aliases: &["pi"],
        effect: RegionEffect::Immediate,
    }
    "math/e" => prim_e {
        doc: "The mathematical constant e (Euler's number).",
        category: "math",
        example: "(math/e)",
        aliases: &["e"],
        effect: RegionEffect::Immediate,
    }
    "math/asin" => prim_asin {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arcsine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/asin 1)",
        aliases: &["asin"],
        effect: RegionEffect::Immediate,
    }
    "math/acos" => prim_acos {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arccosine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/acos 1)",
        aliases: &["acos"],
        effect: RegionEffect::Immediate,
    }
    "math/atan" => prim_atan {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arctangent of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/atan 1)",
        aliases: &["atan"],
        effect: RegionEffect::Immediate,
    }
    "math/fmod" => prim_fmod {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Floating-point remainder. Returns a - floor(a/b) * b.",
        params: &["a", "b"],
        category: "math",
        example: "(math/fmod 5.5 2.0) #=> 1.5",
        aliases: &["fmod"],
        effect: RegionEffect::Immediate,
    }
    "math/atan2" => prim_atan2 {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Returns the arctangent of y/x (in radians), using the signs of both arguments to determine the quadrant.",
        params: &["y", "x"],
        category: "math",
        example: "(math/atan2 1 1)",
        aliases: &["atan2"],
        effect: RegionEffect::Immediate,
    }
    "math/sinh" => prim_sinh {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic sine of a number.",
        params: &["x"],
        category: "math",
        example: "(math/sinh 1)",
        aliases: &["sinh"],
        effect: RegionEffect::Immediate,
    }
    "math/cosh" => prim_cosh {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic cosine of a number.",
        params: &["x"],
        category: "math",
        example: "(math/cosh 1)",
        aliases: &["cosh"],
        effect: RegionEffect::Immediate,
    }
    "math/tanh" => prim_tanh {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic tangent of a number.",
        params: &["x"],
        category: "math",
        example: "(math/tanh 1)",
        aliases: &["tanh"],
        effect: RegionEffect::Immediate,
    }
    "math/log2" => prim_log2 {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the base-2 logarithm of a number.",
        params: &["x"],
        category: "math",
        example: "(math/log2 8)",
        aliases: &["log2"],
        effect: RegionEffect::Immediate,
    }
    "math/log10" => prim_log10 {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the base-10 logarithm of a number.",
        params: &["x"],
        category: "math",
        example: "(math/log10 100)",
        aliases: &["log10"],
        effect: RegionEffect::Immediate,
    }
    "math/trunc" => prim_trunc {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the integer part of a number, truncating toward zero.",
        params: &["x"],
        category: "math",
        example: "(math/trunc 3.7)",
        aliases: &["trunc"],
        effect: RegionEffect::Immediate,
    }
    "math/cbrt" => prim_cbrt {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the cube root of a number.",
        params: &["x"],
        category: "math",
        example: "(math/cbrt 27)",
        aliases: &["cbrt"],
        effect: RegionEffect::Immediate,
    }
    "math/exp2" => prim_exp2 {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns 2 raised to the power of x.",
        params: &["x"],
        category: "math",
        example: "(math/exp2 3)",
        aliases: &["exp2"],
        effect: RegionEffect::Immediate,
    }
    "math/inf" => prim_inf {
        doc: "Positive infinity (IEEE 754).",
        category: "math",
        example: "(math/inf)",
        aliases: &["+inf", "inf"],
        effect: RegionEffect::Immediate,
    }
    "math/-inf" => prim_neg_inf {
        doc: "Negative infinity (IEEE 754).",
        category: "math",
        example: "(math/-inf)",
        aliases: &["-inf"],
        effect: RegionEffect::Immediate,
    }
    "math/nan" => prim_nan {
        doc: "Not-a-number (IEEE 754 NaN).",
        category: "math",
        example: "(math/nan)",
        aliases: &["nan"],
        effect: RegionEffect::Immediate,
    }
    "math/f32-bits" => prim_f32_bits {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the IEEE 754 f32 bit pattern of a number as an integer.",
        params: &["x"],
        category: "math",
        example: "(math/f32-bits 1.0)",
        effect: RegionEffect::Immediate,
    }
    "math/f32-from-bits" => prim_f32_from_bits {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Reinterpret an integer as an IEEE 754 f32 bit pattern.",
        params: &["bits"],
        category: "math",
        example: "(math/f32-from-bits 1065353216)",
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-math.lisp
