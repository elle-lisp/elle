//! I/O primitives: type predicates and backend operations.

use crate::effects::Effect;
use crate::io::backend::SyncBackend;
use crate::io::request::IoRequest;
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

/// (io/backend kind) → backend
fn prim_io_backend(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/backend: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_keyword_name() {
        Some("sync") => {
            let backend = SyncBackend::new();
            (SIG_OK, Value::external("io-backend", backend))
        }
        Some(other) => (
            SIG_ERROR,
            error_val(
                "value-error",
                format!("io/backend: unknown kind :{}, expected :sync", other),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("io/backend: expected keyword, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// (io/execute backend request) → value
fn prim_io_execute(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/execute: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let backend = match args[0].as_external::<SyncBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "io/execute: expected io-backend, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let request = match args[1].as_external::<IoRequest>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "io/execute: expected io-request, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    backend.execute(request)
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "io-request?",
        func: prim_is_io_request,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is an I/O backend.",
        params: &["value"],
        category: "predicate",
        example: "(io-backend? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/backend",
        func: prim_io_backend,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Create an I/O backend. :sync for synchronous.",
        params: &["kind"],
        category: "io",
        example: "(io/backend :sync)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/execute",
        func: prim_io_execute,
        effect: Effect::errors(),
        arity: Arity::Exact(2),
        doc: "Execute an I/O request on a backend. Blocking.",
        params: &["backend", "request"],
        category: "io",
        example: "(io/execute backend request)",
        aliases: &[],
    },
];
