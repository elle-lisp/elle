//! Port primitives — lifecycle management for file descriptors.

use crate::io::request::{IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::kwarg::extract_keyword_timeout;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Helper: extract &Port from a Value, or return a type error.
///
/// Usage in primitives:
/// ```ignore
/// let port = extract_port(&args[0], "port/close")?;
/// ```
fn extract_port<'a>(value: &'a Value, prim_name: &str) -> Result<&'a Port, (SignalBits, Value)> {
    value.as_external::<Port>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        )
    })
}

/// Map an Elle mode keyword name to POSIX open(2) flags and direction.
///
/// All flags include O_CLOEXEC for atomic close-on-exec at openat() time,
/// avoiding the race window between openat() and a post-hoc fcntl().
fn mode_to_flags(mode: &str) -> Option<(i32, Direction)> {
    match mode {
        "read" => Some((libc::O_RDONLY | libc::O_CLOEXEC, Direction::Read)),
        "write" => Some((
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
            Direction::Write,
        )),
        "append" => Some((
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_CLOEXEC,
            Direction::Write,
        )),
        "read-write" => Some((
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC,
            Direction::ReadWrite,
        )),
        _ => None,
    }
}

/// Helper: open a file with the given encoding.
///
/// Shared implementation for `port/open` and `port/open-bytes`.
/// Yields `SIG_YIELD | SIG_IO` with an `IoRequest` containing `IoOp::Open`.
/// Argument validation (path type, mode keyword, timeout) happens here before yielding.
fn open_file(args: &[Value], encoding: Encoding, prim_name: &str) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "{}: expected at least 2 arguments, got {}",
                    prim_name,
                    args.len()
                ),
            ),
        );
    }

    let path = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string for path, got {}",
                        prim_name,
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let mode_name_owned = match args[1].as_keyword_name() {
        Some(name) => name,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected keyword for mode, got {}",
                        prim_name,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    let (flags, direction) = match mode_to_flags(&mode_name_owned) {
        Some(pair) => pair,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: unknown mode :{}, expected :read, :write, :append, or :read-write",
                        prim_name, mode_name_owned
                    ),
                ),
            );
        }
    };

    let timeout = match extract_keyword_timeout(args, 2, prim_name) {
        Ok(t) => t,
        Err(e) => return e,
    };

    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            IoOp::Open {
                path,
                flags,
                mode: 0o666,
                direction,
                encoding,
            },
            Value::NIL,
            timeout,
        ),
    )
}

/// (port/open path mode) → port
///
/// Open a file with text (UTF-8) encoding.
fn prim_port_open(args: &[Value]) -> (SignalBits, Value) {
    open_file(args, Encoding::Text, "port/open")
}

/// (port/open-bytes path mode) → port
///
/// Open a file with binary encoding.
fn prim_port_open_bytes(args: &[Value]) -> (SignalBits, Value) {
    open_file(args, Encoding::Binary, "port/open-bytes")
}

/// (port/close port) → nil
///
/// Close a port. Idempotent — closing an already-closed port is a no-op.
///
/// For ports with an fd (file, network, pipe), yields SIG_IO so the
/// async scheduler can cancel pending io_uring operations before the
/// fd is dropped. For stdio ports (no owned fd) and already-closed
/// ports, completes synchronously.
fn prim_port_close(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/close: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port(&args[0], "port/close") {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Already closed: no-op.
    if port.is_closed() {
        return (SIG_OK, Value::NIL);
    }
    // Stdio ports don't own their fd — close synchronously.
    if !port.has_fd() {
        port.close();
        return (SIG_OK, Value::NIL);
    }
    // Ports with an fd: yield to the I/O scheduler so it can cancel
    // pending operations before the fd is dropped.
    (SIG_YIELD | SIG_IO, IoRequest::new(IoOp::Close, args[0]))
}

/// (port/stdin) → port
fn prim_port_stdin(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/stdin: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::external("port", Port::stdin()))
}

/// (port/stdout) → port
fn prim_port_stdout(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/stdout: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::external("port", Port::stdout()))
}

/// (port/stderr) → port
fn prim_port_stderr(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/stderr: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::external("port", Port::stderr()))
}

/// (port? value) → boolean
fn prim_is_port(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("port")),
    )
}

/// (port/open? port) → boolean
///
/// Returns true if the port is open, false if closed.
/// Signals :type-error if argument is not a port.
fn prim_is_port_open(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/open?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port(&args[0], "port/open?") {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(!port.is_closed()))
}

/// (port/set-options port :timeout ms) → nil
///
/// Set port options. Currently only :timeout is recognized.
/// Pass nil to clear the timeout.
fn prim_port_set_options(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/set-options") {
        Ok(p) => p,
        Err(e) => return e,
    };

    let remaining = &args[1..];
    if !remaining.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                "port/set-options: keyword arguments must be key-value pairs",
            ),
        );
    }

    let mut i = 0;
    while i < remaining.len() {
        let key = &remaining[i];
        let val = &remaining[i + 1];

        match key.as_keyword_name().as_deref() {
            Some("timeout") => {
                if val.is_nil() {
                    port.set_timeout_ms(None);
                } else {
                    match val.as_int() {
                        Some(ms) if ms >= 0 => {
                            port.set_timeout_ms(Some(ms as u64));
                        }
                        Some(ms) => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "value-error",
                                    format!(
                                        "port/set-options: :timeout must be non-negative, got {}",
                                        ms
                                    ),
                                ),
                            );
                        }
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!(
                                        "port/set-options: :timeout value must be integer or nil, got {}",
                                        val.type_name()
                                    ),
                                ),
                            );
                        }
                    }
                }
            }
            Some(other) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("port/set-options: unknown option :{}", other),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "port/set-options: expected keyword, got {}",
                            key.type_name()
                        ),
                    ),
                );
            }
        }
        i += 2;
    }

    (SIG_OK, Value::NIL)
}

/// (port/path port) → string or nil
///
/// Returns the path or address the port was opened on:
/// - File port: the file path string (e.g. "/tmp/foo.txt")
/// - TCP listener: the bound address string (e.g. "127.0.0.1:8080")
/// - TCP stream: the peer address string (e.g. "127.0.0.1:54321")
/// - Stdio ports (stdin/stdout/stderr): nil
///
/// Signals :type-error if argument is not a port.
fn prim_port_path(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/path: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port(&args[0], "port/path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    match port.path() {
        Some(p) => (SIG_OK, Value::string(p)),
        None => (SIG_OK, Value::NIL),
    }
}

/// (port/seek port offset)
/// (port/seek port offset :from :start|:current|:end)
///
/// Seek to `offset` in a file port. Discards the per-fd read buffer before
/// seeking (prevents stale buffered data from diverging from the kernel
/// position). Returns the new absolute byte offset as int.
///
/// The `:from` keyword controls the seek origin:
///   :start   — SEEK_SET (default): absolute offset from file start
///   :current — SEEK_CUR: relative to current position
///   :end     — SEEK_END: relative to end of file (offset is usually negative)
///
/// Only valid on file ports. Returns :type-error on other port kinds.
fn prim_port_seek(args: &[Value]) -> (SignalBits, Value) {
    // Arity: exactly 2 or exactly 4 (port, offset, :from, :value).
    // 0, 1, 3, or 5+ args are all errors.
    if args.len() < 2 || args.len() > 4 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/seek: expected 2 or 4 arguments, got {}", args.len()),
            ),
        );
    }
    if args.len() == 3 {
        return (
            SIG_ERROR,
            error_val("arity-error", "port/seek: :from keyword requires a value"),
        );
    }

    let port = match extract_port(&args[0], "port/seek") {
        Ok(p) => p,
        Err(e) => return e,
    };

    if port.kind() != PortKind::File {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("port/seek: expected file port, got {:?}", port.kind()),
            ),
        );
    }

    let offset = match args[1].as_int() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "port/seek: expected integer for offset, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    // Parse optional :from keyword-value pair (args[2] and args[3]).
    let whence = if args.len() == 4 {
        match args[2].as_keyword_name().as_deref() {
            Some("from") => {}
            Some(other) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("port/seek: unknown keyword :{}, expected :from", other),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "port/seek: expected keyword for third argument, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        }
        match args[3].as_keyword_name().as_deref() {
            Some("start") => libc::SEEK_SET,
            Some("current") => libc::SEEK_CUR,
            Some("end") => libc::SEEK_END,
            Some(other) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!(
                        "port/seek: invalid :from value :{}, expected :start, :current, or :end",
                        other
                    ),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "port/seek: expected keyword for :from value, got {}",
                            args[3].type_name()
                        ),
                    ),
                )
            }
        }
    } else {
        libc::SEEK_SET // default: seek from start
    };

    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(IoOp::Seek { offset, whence }, args[0]),
    )
}

/// (port/tell port) → int
///
/// Return the current logical read position in a file port.
/// Logical position = kernel file offset - buffered-but-unconsumed bytes.
/// Only valid on file ports. Returns :type-error on other port kinds.
fn prim_port_tell(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("port/tell: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let port = match extract_port(&args[0], "port/tell") {
        Ok(p) => p,
        Err(e) => return e,
    };

    if port.kind() != PortKind::File {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("port/tell: expected file port, got {:?}", port.kind()),
            ),
        );
    }

    (SIG_YIELD | SIG_IO, IoRequest::new(IoOp::Tell, args[0]))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "port/open",
        func: prim_port_open,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::AtLeast(2),
        doc: "Open a file as a text (UTF-8) port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open \"data.txt\" :read)\n(port/open \"fifo\" :read :timeout 5000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/open-bytes",
        func: prim_port_open_bytes,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::AtLeast(2),
        doc: "Open a file as a binary port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example:
            "(port/open-bytes \"data.bin\" :read)\n(port/open-bytes \"fifo\" :read :timeout 5000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/close",
        func: prim_port_close,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Close a port. Idempotent. Yields to cancel pending I/O before closing the fd.",
        params: &["port"],
        category: "port",
        example: "(port/close p)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/stdin",
        func: prim_port_stdin,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a port for standard input.",
        params: &[],
        category: "port",
        example: "(port/stdin)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/stdout",
        func: prim_port_stdout,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a port for standard output.",
        params: &[],
        category: "port",
        example: "(port/stdout)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/stderr",
        func: prim_port_stderr,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a port for standard error.",
        params: &[],
        category: "port",
        example: "(port/stderr)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port?",
        func: prim_is_port,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Check if value is a port.",
        params: &["value"],
        category: "predicate",
        example: "(port? (port/stdin)) #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/open?",
        func: prim_is_port_open,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a port is open. Signals :type-error on non-port.",
        params: &["port"],
        category: "port",
        example: "(port/open? (port/stdout)) #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/set-options",
        func: prim_port_set_options,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Set port options. Currently: :timeout ms (nil clears).",
        params: &["port"],
        category: "port",
        example: "(port/set-options p :timeout 5000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/path",
        func: prim_port_path,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the path or address the port was opened on, or nil for stdio ports.",
        params: &["port"],
        category: "port",
        example: "(port/path (tcp/listen \"127.0.0.1\" 0))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/seek",
        func: prim_port_seek,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Range(2, 4),
        doc: "Seek to a byte offset in a file port. Returns new absolute position.\nSyntax: (port/seek port offset [:from :start|:current|:end])\nDefault :from is :start (SEEK_SET). Discards the read buffer on seek.",
        params: &["port", "offset"],
        category: "port",
        example: "(port/seek p 0)\n(port/seek p 0 :from :start)\n(port/seek p -1 :from :end)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/tell",
        func: prim_port_tell,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Return current logical byte position in a file port.\nAccounts for per-fd read buffering: position = kernel_offset - buffer.len().",
        params: &["port"],
        category: "port",
        example: "(port/tell p)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::request::{IoOp, IoRequest};
    use crate::value::fiber::{SIG_IO, SIG_OK, SIG_YIELD};

    fn make_port() -> Value {
        Value::external("port", Port::stdin())
    }

    // ── port/open yield behavior ──────────────────────────────────────────────

    #[test]
    fn test_port_open_yields_sig_io_for_valid_args() {
        let (bits, val) = prim_port_open(&[
            Value::string("/tmp/elle-test-port-open-yield"),
            Value::keyword("write"),
        ]);
        // Must yield, not succeed or error synchronously.
        assert_eq!(
            bits,
            SIG_YIELD | SIG_IO,
            "port/open must yield SIG_YIELD|SIG_IO for valid args"
        );
        // The yielded value must be an IoRequest.
        assert_eq!(
            val.external_type_name(),
            Some("io-request"),
            "yielded value must be an IoRequest"
        );
    }

    #[test]
    fn test_port_open_bytes_yields_sig_io_for_valid_args() {
        let (bits, val) = prim_port_open_bytes(&[
            Value::string("/tmp/elle-test-port-open-bytes-yield"),
            Value::keyword("write"),
        ]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_port_open_iorequest_has_open_op_with_correct_flags() {
        let (bits, val) = prim_port_open(&[
            Value::string("/tmp/test-flags-check"),
            Value::keyword("read"),
        ]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        let req = val.as_external::<IoRequest>().expect("must be IoRequest");
        match &req.op {
            IoOp::Open {
                path,
                flags,
                mode,
                direction,
                encoding,
            } => {
                assert_eq!(path, "/tmp/test-flags-check");
                // O_RDONLY | O_CLOEXEC
                assert!(
                    *flags & libc::O_CLOEXEC != 0,
                    "O_CLOEXEC must be set in flags"
                );
                assert_eq!(
                    *flags & libc::O_WRONLY,
                    0,
                    "O_WRONLY must not be set for :read"
                );
                assert_eq!(*mode, 0o666, "mode must be 0o666");
                assert_eq!(*direction, Direction::Read);
                assert_eq!(*encoding, Encoding::Text);
            }
            _ => panic!("expected IoOp::Open, got {:?}", req.op),
        }
    }

    #[test]
    fn test_port_open_bytes_iorequest_has_binary_encoding() {
        let (bits, val) = prim_port_open_bytes(&[
            Value::string("/tmp/test-encoding-check"),
            Value::keyword("write"),
        ]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        let req = val.as_external::<IoRequest>().expect("must be IoRequest");
        match &req.op {
            IoOp::Open { encoding, .. } => {
                assert_eq!(
                    *encoding,
                    Encoding::Binary,
                    "port/open-bytes must use Binary encoding"
                );
            }
            _ => panic!("expected IoOp::Open"),
        }
    }

    #[test]
    fn test_port_open_write_mode_flags() {
        let (_, val) = prim_port_open(&[
            Value::string("/tmp/test-write-flags"),
            Value::keyword("write"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Open {
                flags, direction, ..
            } => {
                assert!(
                    *flags & libc::O_WRONLY != 0,
                    "O_WRONLY must be set for :write"
                );
                assert!(
                    *flags & libc::O_CREAT != 0,
                    "O_CREAT must be set for :write"
                );
                assert!(
                    *flags & libc::O_TRUNC != 0,
                    "O_TRUNC must be set for :write"
                );
                assert!(
                    *flags & libc::O_CLOEXEC != 0,
                    "O_CLOEXEC must be set for :write"
                );
                assert_eq!(*direction, Direction::Write);
            }
            _ => panic!("expected IoOp::Open"),
        }
    }

    #[test]
    fn test_port_open_append_mode_flags() {
        let (_, val) = prim_port_open(&[
            Value::string("/tmp/test-append-flags"),
            Value::keyword("append"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Open {
                flags, direction, ..
            } => {
                assert!(
                    *flags & libc::O_APPEND != 0,
                    "O_APPEND must be set for :append"
                );
                assert!(
                    *flags & libc::O_CREAT != 0,
                    "O_CREAT must be set for :append"
                );
                assert_eq!(*direction, Direction::Write);
            }
            _ => panic!("expected IoOp::Open"),
        }
    }

    #[test]
    fn test_port_open_read_write_mode_flags() {
        let (_, val) = prim_port_open(&[
            Value::string("/tmp/test-rw-flags"),
            Value::keyword("read-write"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Open {
                flags, direction, ..
            } => {
                assert!(
                    *flags & libc::O_RDWR != 0,
                    "O_RDWR must be set for :read-write"
                );
                assert!(
                    *flags & libc::O_CREAT != 0,
                    "O_CREAT must be set for :read-write"
                );
                assert_eq!(*direction, Direction::ReadWrite);
            }
            _ => panic!("expected IoOp::Open"),
        }
    }

    #[test]
    fn test_port_open_with_timeout_extracts_correctly() {
        let (bits, val) = prim_port_open(&[
            Value::string("/tmp/test-timeout"),
            Value::keyword("read"),
            Value::keyword("timeout"),
            Value::int(5000),
        ]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        let req = val.as_external::<IoRequest>().unwrap();
        assert_eq!(
            req.timeout,
            Some(std::time::Duration::from_millis(5000)),
            "timeout must be extracted from :timeout keyword"
        );
    }

    #[test]
    fn test_port_open_without_timeout_has_none() {
        let (_, val) = prim_port_open(&[
            Value::string("/tmp/test-no-timeout"),
            Value::keyword("read"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        assert_eq!(req.timeout, None, "no timeout keyword → None");
    }

    // ── port/open early-error cases (before yielding) ─────────────────────────

    #[test]
    fn test_port_open_too_few_args_errors() {
        let (bits, _) = prim_port_open(&[Value::string("/tmp/foo")]);
        assert_eq!(bits, SIG_ERROR, "too few args must error before yielding");
    }

    #[test]
    fn test_port_open_no_args_errors() {
        let (bits, _) = prim_port_open(&[]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_open_non_string_path_errors() {
        let (bits, _) = prim_port_open(&[Value::int(42), Value::keyword("read")]);
        assert_eq!(
            bits, SIG_ERROR,
            "non-string path must error before yielding"
        );
    }

    #[test]
    fn test_port_open_bad_mode_errors() {
        let (bits, _) = prim_port_open(&[Value::string("/tmp/foo"), Value::keyword("badmode")]);
        assert_eq!(
            bits, SIG_ERROR,
            "bad mode keyword must error before yielding"
        );
    }

    #[test]
    fn test_port_open_non_keyword_mode_errors() {
        let (bits, _) = prim_port_open(&[Value::string("/tmp/foo"), Value::string("read")]);
        assert_eq!(
            bits, SIG_ERROR,
            "non-keyword mode must error before yielding"
        );
    }

    #[test]
    fn test_port_open_bad_timeout_value_errors() {
        let (bits, _) = prim_port_open(&[
            Value::string("/tmp/foo"),
            Value::keyword("read"),
            Value::keyword("timeout"),
            Value::int(-1),
        ]);
        assert_eq!(
            bits, SIG_ERROR,
            "negative timeout must error before yielding"
        );
    }

    #[test]
    fn test_port_open_unknown_keyword_errors() {
        let (bits, _) = prim_port_open(&[
            Value::string("/tmp/foo"),
            Value::keyword("read"),
            Value::keyword("unknown"),
            Value::int(100),
        ]);
        assert_eq!(
            bits, SIG_ERROR,
            "unknown keyword must error before yielding"
        );
    }

    #[test]
    fn test_port_set_options_timeout() {
        let port_val = make_port();
        let (bits, _) =
            prim_port_set_options(&[port_val, Value::keyword("timeout"), Value::int(5000)]);
        assert_eq!(bits, SIG_OK);
        let port = port_val.as_external::<Port>().unwrap();
        assert_eq!(port.timeout_ms(), Some(5000));
    }

    #[test]
    fn test_port_set_options_clear_timeout() {
        let port_val = make_port();
        prim_port_set_options(&[port_val, Value::keyword("timeout"), Value::int(5000)]);
        let (bits, _) = prim_port_set_options(&[port_val, Value::keyword("timeout"), Value::NIL]);
        assert_eq!(bits, SIG_OK);
        let port = port_val.as_external::<Port>().unwrap();
        assert_eq!(port.timeout_ms(), None);
    }

    #[test]
    fn test_port_set_options_unknown_key_errors() {
        let port_val = make_port();
        let (bits, _) = prim_port_set_options(&[port_val, Value::keyword("foo"), Value::int(1)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_set_options_non_port_errors() {
        let (bits, _) =
            prim_port_set_options(&[Value::int(42), Value::keyword("timeout"), Value::int(1)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_set_options_negative_timeout_errors() {
        let port_val = make_port();
        let (bits, _) =
            prim_port_set_options(&[port_val, Value::keyword("timeout"), Value::int(-1)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_set_options_odd_args_errors() {
        let port_val = make_port();
        let (bits, _) = prim_port_set_options(&[port_val, Value::keyword("timeout")]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_path_file_port() {
        // Create a real file port and check its path
        use std::fs::OpenOptions;
        use std::os::unix::io::OwnedFd;
        let path = "/tmp/elle-test-port-path";
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        let fd: OwnedFd = file.into();
        let port = Port::new_file(
            fd,
            crate::port::Direction::Write,
            crate::port::Encoding::Text,
            path.to_string(),
        );
        let port_val = Value::external("port", port);
        let (bits, result) = prim_port_path(&[port_val]);
        assert_eq!(bits, SIG_OK);
        result
            .with_string(|s| assert_eq!(s, path))
            .expect("expected string result");
    }

    #[test]
    fn test_port_path_stdin_returns_nil() {
        let port_val = Value::external("port", Port::stdin());
        let (bits, result) = prim_port_path(&[port_val]);
        assert_eq!(bits, SIG_OK);
        assert!(result.is_nil());
    }

    #[test]
    fn test_port_path_stdout_returns_nil() {
        let port_val = Value::external("port", Port::stdout());
        let (bits, result) = prim_port_path(&[port_val]);
        assert_eq!(bits, SIG_OK);
        assert!(result.is_nil());
    }

    #[test]
    fn test_port_path_non_port_errors() {
        let (bits, _) = prim_port_path(&[Value::int(42)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_path_wrong_arity_errors() {
        let (bits, _) = prim_port_path(&[]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_path_tcp_listener() {
        use std::net::TcpListener;
        use std::os::unix::io::{FromRawFd, IntoRawFd, OwnedFd};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let fd = unsafe { OwnedFd::from_raw_fd(listener.into_raw_fd()) };
        let port = Port::new_tcp_listener(fd, addr.clone());
        let port_val = Value::external("port", port);
        let (bits, result) = prim_port_path(&[port_val]);
        assert_eq!(bits, SIG_OK);
        result
            .with_string(|s| assert_eq!(s, &addr))
            .expect("expected string result");
    }

    // ── port/seek primitive ──────────────────────────────────────────────────

    fn make_file_port() -> Value {
        // Use a real temp file to test seek behavior more precisely.
        let path = "/tmp/elle-test-primitive-seek-port";
        std::fs::write(path, "hello").unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        Value::external(
            "port",
            Port::new_file(fd, Direction::ReadWrite, Encoding::Text, path.to_string()),
        )
    }

    #[test]
    fn test_port_seek_yields_sig_io() {
        let port = make_file_port();
        let (bits, val) = prim_port_seek(&[port, Value::int(0)]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_port_seek_iorequest_contains_seek_op() {
        let port = make_file_port();
        let (_, val) = prim_port_seek(&[port, Value::int(42)]);
        let req = val.as_external::<IoRequest>().expect("must be IoRequest");
        match &req.op {
            IoOp::Seek { offset, whence } => {
                assert_eq!(*offset, 42);
                assert_eq!(*whence, libc::SEEK_SET, "default whence must be SEEK_SET");
            }
            _ => panic!("expected Seek op"),
        }
    }

    #[test]
    fn test_port_seek_default_whence_is_seek_set() {
        let port = make_file_port();
        let (_, val) = prim_port_seek(&[port, Value::int(0)]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Seek { whence, .. } => assert_eq!(*whence, libc::SEEK_SET),
            _ => panic!("expected Seek"),
        }
    }

    #[test]
    fn test_port_seek_from_current() {
        let port = make_file_port();
        let (_, val) = prim_port_seek(&[
            port,
            Value::int(3),
            Value::keyword("from"),
            Value::keyword("current"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Seek { whence, .. } => assert_eq!(*whence, libc::SEEK_CUR),
            _ => panic!("expected Seek"),
        }
    }

    #[test]
    fn test_port_seek_from_end() {
        let port = make_file_port();
        let (_, val) = prim_port_seek(&[
            port,
            Value::int(-1),
            Value::keyword("from"),
            Value::keyword("end"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Seek { whence, .. } => assert_eq!(*whence, libc::SEEK_END),
            _ => panic!("expected Seek"),
        }
    }

    #[test]
    fn test_port_seek_from_start_explicit() {
        let port = make_file_port();
        let (_, val) = prim_port_seek(&[
            port,
            Value::int(0),
            Value::keyword("from"),
            Value::keyword("start"),
        ]);
        let req = val.as_external::<IoRequest>().unwrap();
        match &req.op {
            IoOp::Seek { whence, .. } => assert_eq!(*whence, libc::SEEK_SET),
            _ => panic!("expected Seek"),
        }
    }

    #[test]
    fn test_port_seek_zero_args_errors() {
        let (bits, _) = prim_port_seek(&[]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_one_arg_errors() {
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[port]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_three_args_errors() {
        // 3 args = incomplete keyword pair (port, offset, :from without value)
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[port, Value::int(0), Value::keyword("from")]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_five_args_errors() {
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[
            port,
            Value::int(0),
            Value::keyword("from"),
            Value::keyword("start"),
            Value::int(99),
        ]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_non_port_arg_errors() {
        let (bits, _) = prim_port_seek(&[Value::int(42), Value::int(0)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_non_file_port_errors() {
        let stdin = Value::external("port", Port::stdin());
        let (bits, _) = prim_port_seek(&[stdin, Value::int(0)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_non_integer_offset_errors() {
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[port, Value::string("not-an-int")]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_bad_from_value_errors() {
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[
            port,
            Value::int(0),
            Value::keyword("from"),
            Value::keyword("bogus"),
        ]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_non_keyword_from_value_errors() {
        let port = make_file_port();
        let (bits, _) =
            prim_port_seek(&[port, Value::int(0), Value::keyword("from"), Value::int(42)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_seek_unknown_first_keyword_errors() {
        // args[2] is a keyword but not :from
        let port = make_file_port();
        let (bits, _) = prim_port_seek(&[
            port,
            Value::int(0),
            Value::keyword("bogus"),
            Value::keyword("start"),
        ]);
        assert_eq!(bits, SIG_ERROR);
    }

    // ── port/tell primitive ──────────────────────────────────────────────────

    #[test]
    fn test_port_tell_yields_sig_io() {
        let port = make_file_port();
        let (bits, val) = prim_port_tell(&[port]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_port_tell_iorequest_contains_tell_op() {
        let port = make_file_port();
        let (_, val) = prim_port_tell(&[port]);
        let req = val.as_external::<IoRequest>().unwrap();
        assert!(matches!(req.op, IoOp::Tell));
    }

    #[test]
    fn test_port_tell_zero_args_errors() {
        let (bits, _) = prim_port_tell(&[]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_tell_two_args_errors() {
        let port = make_file_port();
        let (bits, _) = prim_port_tell(&[port, Value::int(0)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_tell_non_port_arg_errors() {
        let (bits, _) = prim_port_tell(&[Value::int(42)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_port_tell_non_file_port_errors() {
        let stdin = Value::external("port", Port::stdin());
        let (bits, _) = prim_port_tell(&[stdin]);
        assert_eq!(bits, SIG_ERROR);
    }
}
