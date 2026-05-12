use crate::arithmetic;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

pub(crate) fn prim_min(args: &[Value]) -> (SignalBits, Value) {
    if !args[0].is_number() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("min: expected number, got {}", args[0].type_name()),
            ),
        );
    }
    let mut min = args[0];
    for arg in &args[1..] {
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

pub(crate) fn prim_max(args: &[Value]) -> (SignalBits, Value) {
    if !args[0].is_number() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("max: expected number, got {}", args[0].type_name()),
            ),
        );
    }
    let mut max = args[0];
    for arg in &args[1..] {
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

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "min",
        func: prim_min,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Minimum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(min 3 1 4) #=> 1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "max",
        func: prim_max,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Maximum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(max 3 1 4) #=> 4",
        aliases: &[],
    },
];
