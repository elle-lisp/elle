//! Parameter primitives (Racket-style dynamic parameters)

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Create a new parameter with a default value.
/// (parameter default) → parameter
pub(crate) fn prim_make_parameter(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::parameter(args[0]))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "parameter",
        func: prim_make_parameter,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a new dynamic parameter with a default value.",
        params: &["default"],
        category: "parameter",
        example: "(def p (parameter 42))\n(p) #=> 42",
        aliases: &["make-parameter"],
    },
];
