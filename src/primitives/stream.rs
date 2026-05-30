//! Stream primitives — yield SIG_YIELD | SIG_IO with IoRequest descriptors.
//!
//! These primitives do not perform I/O themselves. They build an
//! IoRequest and return (SIG_YIELD | SIG_IO, request), which suspends
//! the fiber. The scheduler catches SIG_IO and dispatches to a backend.

use crate::io::request::{IoOp, IoRequest};
use crate::port::Port;
use crate::primitives::kwarg::extract_keyword_timeout;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Helper: validate that arg is a port.
fn extract_port_value(value: &Value, prim_name: &str) -> Result<Value, (SignalBits, Value)> {
    if value.as_external::<Port>().is_none() {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        ));
    }
    Ok(*value)
}

/// (port/read-line port [:timeout ms]) → bytes | nil
fn prim_stream_read_line(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-line") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/read-line") {
        Ok(t) => t,
        Err(e) => return e,
    };
    let buffer = Value::bytes(vec![0u8; READ_LINE_BUF_SIZE]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::ReadLine { buffer }, port, timeout),
    )
}

/// Default buffer size for ReadLine operations (64KB).
/// Covers every real protocol line. If a line exceeds this, the fiber
/// receives a partial result and can re-issue the read.
const READ_LINE_BUF_SIZE: usize = 65536;

/// (port/read port n [:timeout ms]) → string | bytes | nil
/// Text ports return a string of up to n characters; binary ports return bytes.
fn prim_stream_read(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => return (SIG_OK, Value::bytes(vec![])),
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("port/read: count must be non-negative, got {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "port/read: expected integer for count, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let timeout = match extract_keyword_timeout(args, 2, "port/read") {
        Ok(t) => t,
        Err(e) => return e,
    };
    let buffer = Value::bytes(vec![0u8; count]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Read { count, buffer }, port, timeout),
    )
}

/// (port/read-exact port n [:timeout ms]) → bytes | nil
fn prim_stream_read_exact(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-exact") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => return (SIG_OK, Value::bytes(vec![])),
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("port/read-exact: count must be non-negative, got {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "port/read-exact: expected integer for count, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let timeout = match extract_keyword_timeout(args, 2, "port/read-exact") {
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
    let buffer = Value::bytes(vec![0u8; buf_len]);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::ReadExact { count, buffer }, port, timeout),
    )
}

/// (port/read-all port [:timeout ms]) → string | bytes
fn prim_stream_read_all(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/read-all") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/read-all") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::ReadAll, port, timeout),
    )
}

/// (port/write port data [:timeout ms]) → int
fn prim_stream_write(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/write") {
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
    let timeout = match extract_keyword_timeout(args, 2, "port/write") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Write { data }, port, timeout),
    )
}

/// (port/flush port [:timeout ms]) → nil
fn prim_stream_flush(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/flush") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "port/flush") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Flush, port, timeout),
    )
}

primitive! {
    "port/read-line" => prim_stream_read_line {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(1),
        doc: "Read one line from port. Returns bytes or nil (EOF).",
        params: &["port"],
        category: "port",
        example: "(port/read-line (port/open \"file.txt\" :read))",
        aliases: &["port/read-line"],
    }
    "port/read" => prim_stream_read {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(2),
        doc: "Read up to n bytes from port. Returns bytes or nil (EOF).",
        params: &["port", "n"],
        category: "port",
        example: "(port/read (port/open \"file.txt\" :read) 1024)",
        aliases: &["stream/read"],
    }
    "port/read-exact" => prim_stream_read_exact {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
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
    }
    "port/read-all" => prim_stream_read_all {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(1),
        doc: "Read everything remaining from port.",
        params: &["port"],
        category: "port",
        example: "(port/read-all (port/open \"file.txt\" :read))",
        aliases: &["port/read-all"],
    }
    "port/write" => prim_stream_write {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(2),
        doc: "Write data to port. Returns bytes written.",
        params: &["port", "data"],
        category: "port",
        example: "(port/write (port/stdout) \"hello\")",
        aliases: &["stream/write"],
    }
    "port/flush" => prim_stream_flush {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(1),
        doc: "Flush port's write buffer.",
        params: &["port"],
        category: "port",
        example: "(port/flush (port/stdout))",
        aliases: &["stream/flush"],
    }
}

// Tests migrated to tests/elle/prim-stream.lisp
