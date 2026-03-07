//! Stream primitives — yield SIG_IO with IoRequest descriptors.
//!
//! These primitives do not perform I/O themselves. They build an
//! IoRequest and return (SIG_IO, request), which suspends the fiber.
//! The scheduler catches SIG_IO and dispatches to a backend.

use crate::effects::Effect;
use crate::io::request::{IoOp, IoRequest};
use crate::port::Port;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO};
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

/// (stream/read-line port) → string | nil
fn prim_stream_read_line(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("stream/read-line: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port_value(&args[0], "stream/read-line") {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_IO, IoRequest::new(IoOp::ReadLine, port))
}

/// (stream/read port n) → bytes | nil
fn prim_stream_read(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("stream/read: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port_value(&args[0], "stream/read") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("stream/read: count must be positive, got {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "stream/read: expected integer for count, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_IO, IoRequest::new(IoOp::Read { count }, port))
}

/// (stream/read-all port) → string | bytes
fn prim_stream_read_all(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("stream/read-all: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port_value(&args[0], "stream/read-all") {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_IO, IoRequest::new(IoOp::ReadAll, port))
}

/// (stream/write port data) → int
fn prim_stream_write(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("stream/write: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port_value(&args[0], "stream/write") {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_IO, IoRequest::new(IoOp::Write { data: args[1] }, port))
}

/// (stream/flush port) → nil
fn prim_stream_flush(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("stream/flush: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let port = match extract_port_value(&args[0], "stream/flush") {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_IO, IoRequest::new(IoOp::Flush, port))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "stream/read-line",
        func: prim_stream_read_line,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Read one line from port. Returns string or nil (EOF).",
        params: &["port"],
        category: "stream",
        example: "(stream/read-line (port/open \"file.txt\" :read))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "stream/read",
        func: prim_stream_read,
        effect: Effect::errors(),
        arity: Arity::Exact(2),
        doc: "Read up to n bytes from port. Returns bytes or nil (EOF).",
        params: &["port", "n"],
        category: "stream",
        example: "(stream/read (port/open \"file.txt\" :read) 1024)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "stream/read-all",
        func: prim_stream_read_all,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Read everything remaining from port.",
        params: &["port"],
        category: "stream",
        example: "(stream/read-all (port/open \"file.txt\" :read))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "stream/write",
        func: prim_stream_write,
        effect: Effect::errors(),
        arity: Arity::Exact(2),
        doc: "Write data to port. Returns bytes written.",
        params: &["port", "data"],
        category: "stream",
        example: "(stream/write (port/stdout) \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "stream/flush",
        func: prim_stream_flush,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Flush port's write buffer.",
        params: &["port"],
        category: "stream",
        example: "(stream/flush (port/stdout))",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::Port;
    use crate::value::fiber::SIG_IO;

    #[test]
    fn test_read_line_returns_sig_io() {
        let port_val = Value::external("port", Port::stdin());
        let (bits, val) = prim_stream_read_line(&[port_val]);
        assert_eq!(bits, SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_read_returns_sig_io_with_count() {
        let port_val = Value::external("port", Port::stdin());
        let (bits, val) = prim_stream_read(&[port_val, Value::int(1024)]);
        assert_eq!(bits, SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_write_returns_sig_io() {
        let port_val = Value::external("port", Port::stdout());
        let (bits, val) = prim_stream_write(&[port_val, Value::string("hello")]);
        assert_eq!(bits, SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_non_port_arg_errors() {
        let (bits, _) = prim_stream_read_line(&[Value::int(42)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_wrong_arity_errors() {
        let (bits, _) = prim_stream_read_line(&[]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_read_negative_count_errors() {
        let port_val = Value::external("port", Port::stdin());
        let (bits, _) = prim_stream_read(&[port_val, Value::int(-1)]);
        assert_eq!(bits, SIG_ERROR);
    }
}
