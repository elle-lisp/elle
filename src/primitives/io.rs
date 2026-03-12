//! I/O primitives: type predicates and backend operations.

use crate::io::aio::AsyncBackend;
use crate::io::backend::SyncBackend;
use crate::io::request::IoRequest;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
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
        Some("async") => match AsyncBackend::new() {
            Ok(backend) => (SIG_OK, Value::external("io-backend", backend)),
            Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
        },
        Some(other) => (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "io/backend: unknown kind :{}, expected :sync or :async",
                    other
                ),
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

/// (io/submit backend request) → submission-id
fn prim_io_submit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/submit: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let backend = match args[0].as_external::<AsyncBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/submit: expected async io-backend (created with :async)",
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
                        "io/submit: expected io-request, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    match backend.submit(request) {
        Ok(id) => (SIG_OK, Value::int(id as i64)),
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

/// (io/reap backend) → array-of-completion-structs
fn prim_io_reap(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/reap: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let backend = match args[0].as_external::<AsyncBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/reap: expected async io-backend (created with :async)",
                ),
            )
        }
    };
    let completions = backend.poll();
    let values: Vec<Value> = completions.iter().map(|c| c.to_value()).collect();
    (SIG_OK, Value::array(values))
}

/// (io/wait backend timeout-ms) → array-of-completion-structs
fn prim_io_wait(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/wait: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let backend = match args[0].as_external::<AsyncBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/wait: expected async io-backend (created with :async)",
                ),
            )
        }
    };
    let timeout_ms = match args[1].as_int() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "io/wait: expected integer timeout, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    match backend.wait(timeout_ms) {
        Ok(completions) => {
            let values: Vec<Value> = completions.iter().map(|c| c.to_value()).collect();
            (SIG_OK, Value::array(values))
        }
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "io-request?",
        func: prim_is_io_request,
        signal: Signal::inert(),
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
        signal: Signal::inert(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create an I/O backend. :sync for synchronous, :async for asynchronous.",
        params: &["kind"],
        category: "io",
        example: "(io/backend :sync) (io/backend :async)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/execute",
        func: prim_io_execute,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Execute an I/O request on a backend. Blocking.",
        params: &["backend", "request"],
        category: "io",
        example: "(io/execute backend request)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/submit",
        func: prim_io_submit,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Submit an I/O request to an async backend. Returns submission ID.",
        params: &["backend", "request"],
        category: "io",
        example: "(io/submit backend request)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/reap",
        func: prim_io_reap,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking poll for async I/O completions. Returns array of completion structs.",
        params: &["backend"],
        category: "io",
        example: "(io/reap backend)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "io/wait",
        func: prim_io_wait,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Wait for async I/O completions. timeout-ms: negative=forever, 0=poll, positive=ms. Returns array of completion structs.",
        params: &["backend", "timeout-ms"],
        category: "io",
        example: "(io/wait backend 1000)",
        aliases: &[],
    },
];
