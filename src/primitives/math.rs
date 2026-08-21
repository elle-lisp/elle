//! Math primitives: transcendental functions, constants, and IEEE-754 helpers.
//!
//! Handlers split by shape: uniform number→float ops in `unary`, everything
//! with bespoke argument handling (log/pow/fmod/atan2, constants, f32 bitcasts)
//! in `special`. Both are glob-re-exported so the `primitive!` table below
//! resolves every handler by bare name.
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

mod special;
mod unary;

use special::*;
use unary::*;

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
