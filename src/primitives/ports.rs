//! Port primitives — lifecycle management for file descriptors.

use crate::effects::Effect;
use crate::port::{Direction, Encoding, Port};
use crate::primitives::def::PrimitiveDef;
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
/// Raises :type-error if argument is not a port.
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

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "port/open",
        func: prim_port_open,
        effect: Effect::errors(),
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
        effect: Effect::errors(),
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
        effect: Effect::errors(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a port is open. Raises :type-error on non-port.",
        params: &["port"],
        category: "port",
        example: "(port/open? (port/stdout)) #=> true",
        aliases: &[],
    },
];
