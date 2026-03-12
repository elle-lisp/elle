//! Parameter primitives (Racket-style dynamic parameters)

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create a new parameter with a default value.
/// (parameter default) → parameter
pub(crate) fn prim_make_parameter(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("parameter: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::parameter(args[0]))
}

/// Check if a value is a parameter.
/// (parameter? value) → boolean
pub(crate) fn prim_is_parameter(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("parameter?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_parameter()))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "parameter",
        func: prim_make_parameter,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Create a new dynamic parameter with a default value.",
        params: &["default"],
        category: "parameter",
        example: "(def p (parameter 42))\n(p) #=> 42",
        aliases: &["make-parameter"],
    },
    PrimitiveDef {
        name: "parameter?",
        func: prim_is_parameter,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a dynamic parameter.",
        params: &["value"],
        category: "predicate",
        example: "(parameter? (make-parameter 0)) #=> true\n(parameter? 42) #=> false",
        aliases: &[],
    },
];
