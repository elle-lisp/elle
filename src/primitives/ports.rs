//! Port primitives — lifecycle management for file descriptors.

use crate::port::{Direction, Encoding, Port};
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
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

/// Helper: open a file with the given encoding.
///
/// Shared implementation for `port/open` and `port/open-bytes`.
fn open_file(args: &[Value], encoding: Encoding, prim_name: &str) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 2 arguments, got {}", prim_name, args.len()),
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

    let mode_name = match args[1].as_keyword_name() {
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

    use std::fs::OpenOptions;

    let (opts, direction) = match mode_name {
        "read" => {
            let mut o = OpenOptions::new();
            o.read(true);
            (o, Direction::Read)
        }
        "write" => {
            let mut o = OpenOptions::new();
            o.write(true).create(true).truncate(true);
            (o, Direction::Write)
        }
        "append" => {
            let mut o = OpenOptions::new();
            o.write(true).create(true).append(true);
            (o, Direction::Write)
        }
        "read-write" => {
            let mut o = OpenOptions::new();
            o.read(true).write(true).create(true);
            (o, Direction::ReadWrite)
        }
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: unknown mode :{}, expected :read, :write, :append, or :read-write",
                        prim_name, mode_name
                    ),
                ),
            );
        }
    };

    match opts.open(&path) {
        Ok(file) => {
            // File implements Into<OwnedFd> (stable since Rust 1.63)
            let fd: std::os::unix::io::OwnedFd = file.into();
            let port = Port::new_file(fd, direction, encoding, path);
            (SIG_OK, Value::external("port", port))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("io-error", format!("{}: {}", prim_name, e)),
        ),
    }
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
    port.close();
    (SIG_OK, Value::NIL)
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

        match key.as_keyword_name() {
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

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "port/open",
        func: prim_port_open,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Open a file as a text (UTF-8) port.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open \"data.txt\" :read)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/open-bytes",
        func: prim_port_open_bytes,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Open a file as a binary port.",
        params: &["path", "mode"],
        category: "port",
        example: "(port/open-bytes \"data.bin\" :read)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/close",
        func: prim_port_close,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a port. Idempotent.",
        params: &["port"],
        category: "port",
        example: "(port/close p)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "port/stdin",
        func: prim_port_stdin,
        signal: Signal::inert(),
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
        signal: Signal::inert(),
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
        signal: Signal::inert(),
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
        signal: Signal::inert(),
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
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::fiber::SIG_OK;

    fn make_port() -> Value {
        Value::external("port", Port::stdin())
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
}
