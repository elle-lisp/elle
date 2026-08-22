//! Port primitives — lifecycle management for file descriptors.

use crate::io::request::{IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::primitives::kwarg::extract_keyword_timeout;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;

/// Helper: extract &Port from a Value, or return a type error.
///
/// Usage in primitives:
/// ```ignore
/// let port = extract_port(&args[0], "port/close", ctx)?;
/// ```
fn extract_port<'a>(
    value: &'a Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'a Port, (SignalBits, Value)> {
    value.as_external::<Port>().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        )
    })
}

mod lifecycle;
mod query;
use lifecycle::*;
use query::*;

primitive! {
    "port/open" => prim_port_open {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Open a file as a text (UTF-8) port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open \"data.txt\" :read)\n(port/open \"fifo\" :read :timeout 5000)",
        // Fresh: the port is pre-minted in this call's ctx region (Port::new_unopened)
        // and the completion sets its fd in place, returning the same `*port_val`.
        effect: RegionEffect::Fresh,
    }
    "port/open-bytes" => prim_port_open_bytes {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Open a file as a binary port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open-bytes \"data.bin\" :read)\n(port/open-bytes \"fifo\" :read :timeout 5000)",
        // Fresh: same pre-minted-port-filled-in-place discipline as port/open.
        effect: RegionEffect::Fresh,
    }
    "port/close" => prim_port_close {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Close a port. Idempotent. Yields to cancel pending I/O before closing the fd.",
        params: &["port"],
        category: "port",
        example: "(port/close p)",
        // Immediate: the completion returns Value::NIL. Yields → oracle-exempt.
        effect: RegionEffect::Immediate,
    }
    "port/stdin" => prim_port_stdin { doc: "Return a port for standard input.", category: "port", example: "(port/stdin)", effect: RegionEffect::Fresh, }
    "port/stdout" => prim_port_stdout { doc: "Return a port for standard output.", category: "port", example: "(port/stdout)", effect: RegionEffect::Fresh, }
    "port/stderr" => prim_port_stderr { doc: "Return a port for standard error.", category: "port", example: "(port/stderr)", effect: RegionEffect::Fresh, }
    "port?" => prim_is_port {
        arity: Arity::Exact(1),
        doc: "Check if value is a port.",
        params: &["value"],
        category: "predicate",
        example: "(port? (port/stdin)) #=> true",
        effect: RegionEffect::Immediate,
    }
    "port/open?" => prim_is_port_open {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a port is open. Signals :type-error on non-port.",
        params: &["port"],
        category: "port",
        example: "(port/open? (port/stdout)) #=> true",
        effect: RegionEffect::Immediate,
    }
    "port/set-options" => prim_port_set_options {
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Set port options. Currently: :timeout ms (nil clears).",
        params: &["port"],
        category: "port",
        example: "(port/set-options p :timeout 5000)",
        effect: RegionEffect::Immediate,
    }
    "port/encoding" => prim_port_encoding {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the port's encoding as a keyword: :text or :binary. \
              :text ports return strings and grapheme-count read-exact; \
              :binary ports return bytes and byte-count.",
        params: &["port"],
        category: "port",
        example: "(port/encoding (tcp/connect \"127.0.0.1\" 6379)) #=> :binary",
        aliases: &[],
        effect: RegionEffect::Immediate,
    }
    "port/path" => prim_port_path {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the path or address the port was opened on, or nil for stdio ports.",
        params: &["port"],
        category: "port",
        example: "(port/path (tcp/listen \"127.0.0.1\" 0))",
        effect: RegionEffect::Fresh,
    }
    "port/seek" => prim_port_seek {
        signal: Signal::io_yields_errors(),
        arity: Arity::Range(2, 4),
        doc: "Seek to a byte offset in a file port. Returns new absolute position.\nSyntax: (port/seek port offset [:from :start|:current|:end])\nDefault :from is :start (SEEK_SET). Discards the read buffer on seek.",
        params: &["port", "offset"],
        category: "port",
        example: "(port/seek p 0)\n(port/seek p 0 :from :start)\n(port/seek p -1 :from :end)",
        // Immediate: lseek runs synchronously in the backend and the result is
        // the new position `Value::int(..)`. Still a signal-carrying return → oracle-exempt.
        effect: RegionEffect::Immediate,
    }
    "port/tell" => prim_port_tell {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Return current logical byte position in a file port.\nAccounts for per-fd read buffering: position = kernel_offset - buffer.len().",
        params: &["port"],
        category: "port",
        example: "(port/tell p)",
        // Immediate: the logical position `Value::int(..)` (synchronous lseek).
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-ports.lisp
