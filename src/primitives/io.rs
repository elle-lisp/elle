//! I/O primitives: type predicates and backend operations.

use crate::io::aio::AsyncBackend;
use crate::io::mock::MockBackend;
use crate::io::request::IoRequest;
use crate::io::AnyBackend;
use crate::io::SubmissionId;
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;

/// (io-request? value) → boolean
fn prim_is_io_request(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("io-request")),
    )
}

/// (io-backend? value) → boolean
fn prim_is_io_backend(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("io-backend")),
    )
}

/// (io/backend kind) → backend
fn prim_io_backend(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_keyword_name().as_deref() {
        Some("async") => match AsyncBackend::new() {
            Ok(backend) => {
                let any = AnyBackend(Box::new(backend));
                (SIG_OK, ctx.external("io-backend", any))
            }
            Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
        },
        Some("mock") => {
            let any = AnyBackend(Box::new(MockBackend::new()));
            (SIG_OK, ctx.external("io-backend", any))
        }
        Some(other) => (
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!(
                    "io/backend: unknown kind :{}, expected :async or :mock",
                    other
                ),
            ),
        ),
        None => type_error!(ctx, args[0], "io/backend", "keyword"),
    }
}

/// `(io/submit backend request [fiber])` → submission-id
///
/// Optional third arg: the fiber that issued the I/O request. When present,
/// spawn results are allocated on the fiber's heap (eliminating cross-heap
/// references). Without a fiber arg, the current heap is used.
fn prim_io_submit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "io/submit: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let request = match args[1].as_external::<IoRequest>() {
        Some(r) => r,
        None => return type_error!(ctx, args[1], "io/submit", "io-request"),
    };
    // The requesting instance's own heap (the ctx's). The backend records it and
    // builds every completion value on it — immediate (spawn) or harvested on the
    // scheduler thread — so results live where the issuing fiber lives, never on a
    // per-thread slot a second instance could share.
    let origin_heap = ctx.heap_mut() as *mut crate::value::fiberheap::FiberHeap;
    match backend.0.submit(request, origin_heap) {
        Ok(id) => (SIG_OK, Value::int(id.as_u64() as i64)),
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

/// (io/reap backend) → array-of-completion-structs
fn prim_io_reap(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "io/reap: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let completions = backend.0.poll();
    // The completion-wrapper structs are born on this instance's own heap (the
    // native call's), matching where the inner result/error values were built.
    let heap: *mut crate::value::fiberheap::FiberHeap = ctx.heap_mut();
    let values: Vec<Value> = completions.iter().map(|c| c.to_value(heap)).collect();
    (SIG_OK, ctx.array(values))
}

/// (io/wait backend timeout-ms) → array-of-completion-structs
fn prim_io_wait(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "io/wait: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let timeout_ms = prim_arg!(ctx, args, 1, as_int, "io/wait", "integer timeout");
    match backend.0.wait(timeout_ms) {
        Ok(completions) => {
            let heap: *mut crate::value::fiberheap::FiberHeap = ctx.heap_mut();
            let values: Vec<Value> = completions.iter().map(|c| c.to_value(heap)).collect();
            (SIG_OK, ctx.array(values))
        }
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

/// (io/cancel backend submission-id) → nil
fn prim_io_cancel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let backend = match args[0].as_external::<AnyBackend>() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "io/cancel: expected async io-backend (created with :async or :mock)",
                ),
            )
        }
    };
    let id = match args[1].as_int() {
        Some(n) if n >= 0 => SubmissionId::from_raw(n as u64),
        _ => {
            return type_error!(
                ctx,
                args[1],
                "io/cancel",
                "non-negative integer submission ID"
            )
        }
    };
    match backend.0.cancel(id) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

// ── Scheduler-yielding I/O primitives ────────────────────────────────

/// Async sleep — yields to the scheduler with a timer IoRequest.
/// (ev/sleep seconds)
fn prim_ev_sleep(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::io::request::IoOp;
    use std::time::Duration;

    let duration = if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                ctx.error("argument-error", "ev/sleep: duration must be non-negative"),
            );
        }
        Duration::from_secs(n as u64)
    } else if let Some(f) = args[0].as_float() {
        if f < 0.0 || !f.is_finite() {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    "ev/sleep: duration must be a finite non-negative number",
                ),
            );
        }
        Duration::from_secs_f64(f)
    } else {
        return (
            SIG_ERROR,
            ctx.error("type-error", "ev/sleep: argument must be a number"),
        );
    };

    (
        SIG_YIELD | SIG_IO,
        IoRequest::portless(ctx, IoOp::Sleep { duration }),
    )
}

/// Poll a raw fd for readiness — yields to the scheduler.
/// (ev/poll-fd fd mode) or (ev/poll-fd fd mode timeout)
/// mode: :read, :write, or :read-write
/// timeout: seconds (float/int), default no timeout
/// Returns revents mask as int, or 0 on timeout.
fn prim_ev_poll_fd(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use std::time::Duration;

    let fd = match args[0].as_int() {
        Some(n) if n >= 0 => n as std::os::unix::io::RawFd,
        _ => {
            return (
                SIG_ERROR,
                ctx.error(
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
                    ctx.error(
                        "argument-error",
                        "ev/poll-fd: mode must be :read, :write, or :read-write",
                    ),
                )
            }
        }
    } else {
        return (
            SIG_ERROR,
            ctx.error("type-error", "ev/poll-fd: mode must be a keyword"),
        );
    };

    let timeout = if args.len() == 3 {
        let secs = if let Some(n) = args[2].as_int() {
            if n < 0 {
                return (
                    SIG_ERROR,
                    ctx.error("argument-error", "ev/poll-fd: timeout must be non-negative"),
                );
            }
            n as f64
        } else if let Some(f) = args[2].as_float() {
            if f < 0.0 || !f.is_finite() {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        "ev/poll-fd: timeout must be a finite non-negative number",
                    ),
                );
            }
            f
        } else {
            return (
                SIG_ERROR,
                ctx.error("type-error", "ev/poll-fd: timeout must be a number"),
            );
        };
        Some(Duration::from_secs_f64(secs))
    } else {
        None
    };

    match timeout {
        Some(t) => (
            SIG_YIELD | SIG_IO,
            IoRequest::poll_fd_with_timeout(ctx, fd, events, t),
        ),
        None => (SIG_YIELD | SIG_IO, IoRequest::poll_fd(ctx, fd, events)),
    }
}

primitive! {
    "io-request?" => prim_is_io_request {
        arity: Arity::Exact(1),
        doc: "Check if value is an I/O request.",
        params: &["value"],
        category: "predicate",
        example: "(io-request? 42) #=> false",
        effect: RegionEffect::Immediate,
    }
    "io-backend?" => prim_is_io_backend {
        arity: Arity::Exact(1),
        doc: "Check if value is an I/O backend.",
        params: &["value"],
        category: "predicate",
        example: "(io-backend? 42) #=> false",
        effect: RegionEffect::Immediate,
    }
    "io/backend" => prim_io_backend {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create an I/O backend. :async for asynchronous, :mock for testing.",
        params: &["kind"],
        category: "io",
        example: "(io/backend :async)",
        effect: RegionEffect::Fresh,
    }
    "io/submit" => prim_io_submit {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Submit an I/O request to an async backend. Optional third arg is the origin fiber for heap-correct spawn allocation. Returns submission ID.",
        params: &["backend", "request"],
        category: "io",
        example: "(io/submit backend request)",
        effect: RegionEffect::Immediate,
    }
    "io/reap" => prim_io_reap {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking poll for async I/O completions. Returns array of completion structs.",
        params: &["backend"],
        category: "io",
        example: "(io/reap backend)",
        effect: RegionEffect::Fresh,
    }
    "io/wait" => prim_io_wait {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Wait for async I/O completions. timeout-ms: negative=forever, 0=poll, positive=ms. Returns array of completion structs.",
        params: &["backend", "timeout-ms"],
        category: "io",
        example: "(io/wait backend 1000)",
        // Fresh: builds a fresh array of completion structs in this call's ctx
        // region, exactly like its sibling io/reap. Synchronous (Signal::errors,
        // no yield), so this Fresh claim is oracle-CHECKED on every debug call.
        effect: RegionEffect::Fresh,
        // The success result is always an immutable array (`ctx.array`), so a
        // binding from it is statically `:array` — the scheduler's
        // `(each c in (io/wait …) …)` prunes its off-array arms (typeinfer/prune.rs).
        ret: RetType::Array,
    }
    "io/cancel" => prim_io_cancel {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Cancel a pending async I/O operation by submission ID. Returns nil.",
        params: &["backend", "id"],
        category: "io",
        example: "(io/cancel backend id)",
        effect: RegionEffect::Immediate,
    }
    "ev/sleep" => prim_ev_sleep {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Async sleep — yields to the scheduler for the specified duration in seconds",
        params: &["seconds"],
        category: "scheduler",
        example: "(ev/sleep 0.5)",
        // Immediate: the Sleep completion returns Value::NIL (no heap args at all).
        effect: RegionEffect::Immediate,
    }
    "ev/poll-fd" => prim_ev_poll_fd {
        signal: Signal::io_yields_errors(),
        arity: Arity::Range(2, 3),
        doc: "Poll a raw fd for readiness — yields to the scheduler. mode: :read, :write, :read-write. Optional timeout in seconds.",
        params: &["fd", "mode", "timeout?"],
        category: "scheduler",
        example: "(ev/poll-fd 5 :read 1.0)",
        // Immediate: the PollFd completion returns the revents mask Value::int(..).
        effect: RegionEffect::Immediate,
    }
}
