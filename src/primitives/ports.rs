//! Port primitives — lifecycle management for file descriptors.

use crate::io::request::{IoOp, IoRequest};
use crate::port::{Direction, Encoding, Port, PortKind};
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

    let port_val = Value::external("port", Port::new_unopened(PortKind::File, direction, encoding, path.clone()));
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
            port_val,
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
    let port = match extract_port(&args[0], "port/close") {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Already closed: no-op.
    if port.is_closed() {
        return (SIG_OK, Value::NIL);
    }
    // Stdout/stderr don't own their fd and have no async resource to
    // tear down — close synchronously. Stdin DOES have an async
    // resource: a dedicated worker thread parked in `libc::read(0, …)`.
    // Even though the port doesn't own fd 0, the close must reach the
    // AsyncBackend so the worker is signalled out of its blocking read
    // and the fiber waiting on the in-flight read is resumed with a
    // `stdin closed` error. See `docs/io.md` and the close branch in
    // `AsyncBackend::submit` for the full path.
    if !port.has_fd() && !matches!(port.kind(), crate::port::PortKind::Stdin) {
        port.close();
        return (SIG_OK, Value::NIL);
    }
    // Stdin and ports with an owned fd: yield to the I/O scheduler so
    // it can cancel pending operations / shut down the stdin worker
    // before the fd is dropped (or, for stdin, the port is marked
    // closed and any in-flight read is woken).
    (SIG_YIELD | SIG_IO, IoRequest::new(IoOp::Close, args[0]))
}

/// (port/stdin) → port
fn prim_port_stdin(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::external("port", Port::stdin()))
}

/// (port/stdout) → port
fn prim_port_stdout(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::external("port", Port::stdout()))
}

/// (port/stderr) → port
fn prim_port_stderr(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::external("port", Port::stderr()))
}

/// (port? value) → boolean
fn prim_is_port(args: &[Value]) -> (SignalBits, Value) {
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
    let port = match extract_port(&args[0], "port/path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    match port.path() {
        Some(p) => (SIG_OK, Value::string(p)),
        None => (SIG_OK, Value::NIL),
    }
}

/// (port/encoding port) → :text | :binary
///
/// Returns the port's encoding mode as a keyword.  `:text` ports return
/// strings from read operations and treat `port/read-exact`'s count as
/// graphemes (the unit Elle strings are measured in).  `:binary` ports
/// return bytes and treat the count as bytes — what byte-framed
/// protocols (RESP, gRPC, HTTP/2, length-prefixed everything) need.
///
/// Use this to guard protocol-implementing code that requires one or
/// the other.  Example: a RESP reader can assert
/// `(= :binary (port/encoding port))` up front and fail with a clear
/// error instead of silently corrupting bulk-string framing.
fn prim_port_encoding(args: &[Value]) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/encoding") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let kw = match port.encoding() {
        crate::port::Encoding::Text => "text",
        crate::port::Encoding::Binary => "binary",
    };
    (SIG_OK, Value::keyword(kw))
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

primitive! {
    "port/open" => prim_port_open {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(2),
        doc: "Open a file as a text (UTF-8) port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open \"data.txt\" :read)\n(port/open \"fifo\" :read :timeout 5000)",
    }
    "port/open-bytes" => prim_port_open_bytes {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::AtLeast(2),
        doc: "Open a file as a binary port. Accepts optional :timeout ms keyword.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open-bytes \"data.bin\" :read)\n(port/open-bytes \"fifo\" :read :timeout 5000)",
    }
    "port/close" => prim_port_close {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::Exact(1),
        doc: "Close a port. Idempotent. Yields to cancel pending I/O before closing the fd.",
        params: &["port"],
        category: "port",
        example: "(port/close p)",
    }
    "port/stdin" => prim_port_stdin { doc: "Return a port for standard input.", category: "port", example: "(port/stdin)", }
    "port/stdout" => prim_port_stdout { doc: "Return a port for standard output.", category: "port", example: "(port/stdout)", }
    "port/stderr" => prim_port_stderr { doc: "Return a port for standard error.", category: "port", example: "(port/stderr)", }
    "port?" => prim_is_port {
        arity: Arity::Exact(1),
        doc: "Check if value is a port.",
        params: &["value"],
        category: "predicate",
        example: "(port? (port/stdin)) #=> true",
    }
    "port/open?" => prim_is_port_open {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a port is open. Signals :type-error on non-port.",
        params: &["port"],
        category: "port",
        example: "(port/open? (port/stdout)) #=> true",
    }
    "port/set-options" => prim_port_set_options {
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Set port options. Currently: :timeout ms (nil clears).",
        params: &["port"],
        category: "port",
        example: "(port/set-options p :timeout 5000)",
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
    }
    "port/path" => prim_port_path {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the path or address the port was opened on, or nil for stdio ports.",
        params: &["port"],
        category: "port",
        example: "(port/path (tcp/listen \"127.0.0.1\" 0))",
    }
    "port/seek" => prim_port_seek {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::Range(2, 4),
        doc: "Seek to a byte offset in a file port. Returns new absolute position.\nSyntax: (port/seek port offset [:from :start|:current|:end])\nDefault :from is :start (SEEK_SET). Discards the read buffer on seek.",
        params: &["port", "offset"],
        category: "port",
        example: "(port/seek p 0)\n(port/seek p 0 :from :start)\n(port/seek p -1 :from :end)",
    }
    "port/tell" => prim_port_tell {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        }),
        arity: Arity::Exact(1),
        doc: "Return current logical byte position in a file port.\nAccounts for per-fd read buffering: position = kernel_offset - buffer.len().",
        params: &["port"],
        category: "port",
        example: "(port/tell p)",
    }
}

// Tests migrated to tests/elle/prim-ports.lisp
