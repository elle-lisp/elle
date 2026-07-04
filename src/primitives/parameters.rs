//! Parameter primitives (Racket-style dynamic parameters)

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Create a new parameter with a default value.
/// (parameter default) → parameter
pub(crate) fn prim_make_parameter(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.parameter(args[0]))
}

primitive! {
    "parameter" => prim_make_parameter {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a new dynamic parameter with a default value.",
        params: &["default"],
        category: "parameter",
        example: "(def p (parameter 42))\n(p) #=> 42",
        aliases: &["make-parameter"],
        effect: RegionEffect::Fresh,
    }
}
