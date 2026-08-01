//! Stream primitives — yield SIG_YIELD | SIG_IO with IoRequest descriptors.
//!
//! These primitives do not perform I/O themselves. They build an
//! IoRequest and return (SIG_YIELD | SIG_IO, request), which suspends
//! the fiber. The scheduler catches SIG_IO and dispatches to a backend.

use crate::io::request::{IoOp, IoRequest};
use crate::port::Port;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::primitives::kwarg::extract_keyword_timeout;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;

/// Helper: validate that arg is a port.
fn extract_port_value(
    value: &Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Value, (SignalBits, Value)> {
    if value.as_external::<Port>().is_none() {
        return Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        ));
    }
    Ok(*value)
}

/// (port/read-line port [:timeout ms]) → bytes | nil
fn prim_stream_read_line(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-line", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/read-line", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let buffer = ctx.bytes(vec![0u8; READ_LINE_BUF_SIZE]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::ReadLine { buffer }, port, timeout),
    )
}

/// Default buffer size for ReadLine operations (64KB).
/// Covers every real protocol line. If a line exceeds this, the fiber
/// receives a partial result and can re-issue the read.
const READ_LINE_BUF_SIZE: usize = 65536;

/// (port/read port n [:timeout ms]) → string | bytes | nil
/// Text ports return a string of up to n characters; binary ports return bytes.
fn prim_stream_read(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => return (SIG_OK, ctx.bytes(vec![])),
        Some(n) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("port/read: count must be non-negative, got {}", n),
                ),
            )
        }
        None => return type_error!(ctx, args[1], "port/read", "integer for count"),
    };
    let timeout = match extract_keyword_timeout(args, 2, "port/read", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let buffer = ctx.bytes(vec![0u8; count]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::Read { count, buffer }, port, timeout),
    )
}

/// (port/read-exact port n [:timeout ms]) → bytes | nil
fn prim_stream_read_exact(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-exact", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => return (SIG_OK, ctx.bytes(vec![])),
        Some(n) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("port/read-exact: count must be non-negative, got {}", n),
                ),
            )
        }
        None => return type_error!(ctx, args[1], "port/read-exact", "integer for count"),
    };
    let timeout = match extract_keyword_timeout(args, 2, "port/read-exact", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    // `count` is the number of *units* to read: bytes on a binary port,
    // grapheme clusters on a text port. A grapheme can span several
    // UTF-8 bytes, so a text read reserves 4 bytes per grapheme (the
    // UTF-8 codepoint upper bound, ample for the common case) so the
    // backend can fill it in one read; the completion path splits at the
    // Nth grapheme boundary and stashes any remainder for the next read.
    // The buffer is allocated in the caller's region so the resulting
    // string stays in-region (bytes_to_string_in_place).
    let is_text = port
        .as_external::<Port>()
        .map(|p| p.encoding() == crate::port::Encoding::Text)
        .unwrap_or(false);
    let buf_len = if is_text {
        count.saturating_mul(4)
    } else {
        count
    };
    let buffer = ctx.bytes(vec![0u8; buf_len]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::ReadExact { count, buffer }, port, timeout),
    )
}

/// (port/read-all port [:timeout ms]) → string | bytes
fn prim_stream_read_all(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-all", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/read-all", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::ReadAll, port, timeout),
    )
}

/// (port/write port data [:timeout ms]) → int
fn prim_stream_write(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/write", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Short-circuit empty writes to avoid unnecessary I/O.
    let data = args[1];
    let is_empty = data.with_string(|s| s.is_empty()).unwrap_or(false)
        || data.as_bytes().is_some_and(|b| b.is_empty())
        || data.as_bytes_mut().is_some_and(|b| b.borrow().is_empty())
        || data.as_string_mut().is_some_and(|b| b.borrow().is_empty());
    if is_empty {
        return (SIG_OK, Value::int(0));
    }
    let timeout = match extract_keyword_timeout(args, 2, "port/write", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::Write { data }, port, timeout),
    )
}

/// (port/flush port [:timeout ms]) → nil
fn prim_stream_flush(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/flush", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/flush", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::Flush, port, timeout),
    )
}

primitive! {
    "port/read-line" => prim_stream_read_line {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Read one line from port. Returns bytes or nil (EOF).",
        params: &["port"],
        category: "port",
        example: "(port/read-line (port/open \"file.txt\" :read))",
        aliases: &["port/read-line"],
        // Fresh: resumes with the read buffer pre-minted in this call's ctx
        // region (filled in place, `bytes_to_string_in_place` keeps it in-region)
        // or nil at EOF. Yields → oracle-exempt; guarded by the io-pass solver
        // test + region-io-effect-pass.lisp.
        effect: RegionEffect::Fresh,
    }
    "port/read" => prim_stream_read {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Read up to n bytes from port. Returns bytes or nil (EOF).",
        params: &["port", "n"],
        category: "port",
        example: "(port/read (port/open \"file.txt\" :read) 1024)",
        aliases: &["stream/read"],
        // Fresh: resumes with the read buffer pre-minted in this call's ctx
        // region (filled in place) or nil at EOF; the `count==0` SIG_OK path
        // returns fresh empty bytes (oracle-checked). See region-io-effect-pass.lisp.
        effect: RegionEffect::Fresh,
    }
    "port/read-exact" => prim_stream_read_exact {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Read exactly n bytes from port, looping over short reads. \
              Returns bytes/string of length n, or nil if EOF arrived first. \
              Unlike port/read (which is 'up to n' per POSIX), this resubmits \
              short reads on stream sockets too — use it for length-prefixed \
              binary framing (RESP, gRPC, h2 DATA, etc.).",
        params: &["port", "n"],
        category: "port",
        example: "(port/read-exact tcp-sock 1024)",
        aliases: &[],
        // Fresh: resumes with the read buffer pre-minted in this call's ctx
        // region (grapheme-split in place for text ports) or nil at EOF.
        effect: RegionEffect::Fresh,
    }
    "port/read-all" => prim_stream_read_all {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Read everything remaining from port.",
        params: &["port"],
        category: "port",
        example: "(port/read-all (port/open \"file.txt\" :read))",
        aliases: &["port/read-all"],
        // Opaque: stores nothing, but unlike the sized reads it has no
        // pre-allocated buffer — the result bytes are minted at completion on the
        // origin heap (`Alloc::new(completion_heap_ptr(..)).bytes(all)`), neither
        // this call's region nor an arg's. No clique, non-fresh result.
        effect: RegionEffect::Opaque,
    }
    "port/write" => prim_stream_write {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Write all of data to port, looping over short writes. \
              Returns the number of bytes written, which equals the length \
              of data — a caller never loops on the count. Errors if the \
              fd fails part-way, since an unknown prefix reached the peer.",
        params: &["port", "data"],
        category: "port",
        example: "(port/write (port/stdout) \"hello\")",
        aliases: &["stream/write"],
        // Every non-error path yields an integer byte count (the empty-write
        // short-circuit returns `Value::int(0)` directly; the io completion
        // returns `Value::int(result_code)`), so the result is always an
        // immediate. `Immediate` records no may-store edges — `port/write`
        // takes two heap args (port + data) but stores neither, so the `Mixed`
        // arg clique only leaked the data region per call. Pinned by
        // region-port-write-effect.lisp (resumed value) and effects.rs
        // `port_write_declares_immediate_no_arg_clique` (no clique). The result
        // side is oracle-checked on the `SIG_OK` empty-write path; the yield
        // path is oracle-exempt.
        effect: RegionEffect::Immediate,
    }
    "port/flush" => prim_stream_flush {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Flush port's write buffer.",
        params: &["port"],
        category: "port",
        example: "(port/flush (port/stdout))",
        aliases: &["stream/flush"],
        // Immediate: the io completion returns Value::NIL. Yields → oracle-exempt.
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-stream.lisp
