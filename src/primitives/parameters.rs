//! Parameter primitives (Racket-style dynamic parameters)

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create a new parameter with a default value.
/// (make-parameter default) → parameter
pub fn prim_make_parameter(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("make-parameter: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::parameter(args[0]))
}

/// Check if a value is a parameter.
/// (parameter? value) → boolean
pub fn prim_is_parameter(args: &[Value]) -> (SignalBits, Value) {
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

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "make-parameter",
        func: prim_make_parameter,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Create a new dynamic parameter with a default value.",
        params: &["default"],
        category: "parameter",
        example: "(def p (make-parameter 42))\n(p) #=> 42",
        aliases: &[],
    },
    PrimitiveDef {
        name: "parameter?",
        func: prim_is_parameter,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a dynamic parameter.",
        params: &["value"],
        category: "predicate",
        example: "(parameter? (make-parameter 0)) #=> true\n(parameter? 42) #=> false",
        aliases: &[],
    },
];
