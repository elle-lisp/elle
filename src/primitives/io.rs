//! I/O primitives: type predicates and backend operations.

use crate::io::aio::AsyncBackend;
use crate::io::mock::MockBackend;
use crate::io::request::IoRequest;
use crate::io::AnyBackend;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
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
    match args[0].as_keyword_name().as_deref() {
        Some("async") => match AsyncBackend::new() {
            Ok(backend) => {
                let any = AnyBackend(Box::new(backend));
                (SIG_OK, Value::external("io-backend", any))
            }
            Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
        },
        Some("mock") => {
            let any = AnyBackend(Box::new(MockBackend::new()));
            (SIG_OK, Value::external("io-backend", any))
        }
        Some(other) => (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "io/backend: unknown kind :{}, expected :async or :mock",
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
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/submit: expected async io-backend (created with :async or :mock)",
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
    match backend.0.submit(request) {
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
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/reap: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let completions = backend.0.poll();
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
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/wait: expected async io-backend (created with :async or :mock)",
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
    match backend.0.wait(timeout_ms) {
        Ok(completions) => {
            let values: Vec<Value> = completions.iter().map(|c| c.to_value()).collect();
            (SIG_OK, Value::array(values))
        }
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

/// (io/cancel backend submission-id) → nil
fn prim_io_cancel(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("io/cancel: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "io/cancel: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let id = match args[1].as_int() {
        Some(n) if n >= 0 => n as u64,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "io/cancel: expected non-negative integer submission ID, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    match backend.0.cancel(id) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

// ── Scheduler-yielding I/O primitives ────────────────────────────────

/// Async sleep — yields to the scheduler with a timer IoRequest.
/// (ev/sleep seconds)
fn prim_ev_sleep(args: &[Value]) -> (SignalBits, Value) {
    use crate::io::request::IoOp;
    use std::time::Duration;

    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("ev/sleep: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let duration = if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                error_val("argument-error", "ev/sleep: duration must be non-negative"),
            );
        }
        Duration::from_secs(n as u64)
    } else if let Some(f) = args[0].as_float() {
        if f < 0.0 || !f.is_finite() {
            return (
                SIG_ERROR,
                error_val(
                    "argument-error",
                    "ev/sleep: duration must be a finite non-negative number",
                ),
            );
        }
        Duration::from_secs_f64(f)
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", "ev/sleep: argument must be a number"),
        );
    };

    (
        SIG_YIELD | SIG_IO,
        IoRequest::portless(IoOp::Sleep { duration }),
    )
}

/// Poll a raw fd for readiness — yields to the scheduler.
/// (ev/poll-fd fd mode) or (ev/poll-fd fd mode timeout)
/// mode: :read, :write, or :read-write
/// timeout: seconds (float/int), default no timeout
/// Returns revents mask as int, or 0 on timeout.
fn prim_ev_poll_fd(args: &[Value]) -> (SignalBits, Value) {
    use std::time::Duration;

    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("ev/poll-fd: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let fd = match args[0].as_int() {
        Some(n) if n >= 0 => n as std::os::unix::io::RawFd,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "ev/poll-fd: fd must be a non-negative integer",
                ),
            )
        }
    };

    let events: u32 = if let Some(kw) = args[1].as_keyword_name() {
        match kw.as_str() {
            "read" => libc::POLLIN as u32,
            "write" => libc::POLLOUT as u32,
            "read-write" => (libc::POLLIN | libc::POLLOUT) as u32,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        "ev/poll-fd: mode must be :read, :write, or :read-write",
                    ),
                )
            }
        }
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", "ev/poll-fd: mode must be a keyword"),
        );
    };

    let timeout = if args.len() == 3 {
        let secs = if let Some(n) = args[2].as_int() {
            if n < 0 {
                return (
                    SIG_ERROR,
                    error_val("argument-error", "ev/poll-fd: timeout must be non-negative"),
                );
            }
            n as f64
        } else if let Some(f) = args[2].as_float() {
            if f < 0.0 || !f.is_finite() {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        "ev/poll-fd: timeout must be a finite non-negative number",
                    ),
                );
            }
            f
        } else {
            return (
                SIG_ERROR,
                error_val("type-error", "ev/poll-fd: timeout must be a number"),
            );
        };
        Some(Duration::from_secs_f64(secs))
    } else {
        None
    };

    match timeout {
        Some(t) => (
            SIG_YIELD | SIG_IO,
            IoRequest::poll_fd_with_timeout(fd, events, t),
        ),
        None => (SIG_YIELD | SIG_IO, IoRequest::poll_fd(fd, events)),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "io-request?",
        func: prim_is_io_request,
        signal: Signal::silent(),
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
        signal: Signal::silent(),
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
        doc: "Create an I/O backend. :async for asynchronous, :mock for testing.",
        params: &["kind"],
        category: "io",
        example: "(io/backend :async)",
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
    PrimitiveDef {
        name: "io/cancel",
        func: prim_io_cancel,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Cancel a pending async I/O operation by submission ID. Returns nil.",
        params: &["backend", "id"],
        category: "io",
        example: "(io/cancel backend id)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ev/sleep",
        func: prim_ev_sleep,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Async sleep — yields to the scheduler for the specified duration in seconds",
        params: &["seconds"],
        category: "scheduler",
        example: "(ev/sleep 0.5)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ev/poll-fd",
        func: prim_ev_poll_fd,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Range(2, 3),
        doc: "Poll a raw fd for readiness — yields to the scheduler. mode: :read, :write, :read-write. Optional timeout in seconds.",
        params: &["fd", "mode", "timeout?"],
        category: "scheduler",
        example: "(ev/poll-fd 5 :read 1.0)",
        aliases: &[],
    },
];
