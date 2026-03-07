//! I/O primitives: type predicates and backend operations.

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (io-request? value) → boolean
fn prim_is_io_request(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io-request?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("io-request")),
    )
}

/// (io-backend? value) → boolean
fn prim_is_io_backend(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io-backend?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("io-backend")),
    )
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "io-request?",
        func: prim_is_io_request,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is an I/O request.",
        params: &["value"],
        category: "predicate",
        example: "(io-request? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io-backend?",
        func: prim_is_io_backend,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is an I/O backend.",
        params: &["value"],
        category: "predicate",
        example: "(io-backend? 42) #=> false",
        aliases: &[],
    },
];
