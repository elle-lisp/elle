//! IoRequest — typed I/O request descriptors.
//!
//! Stream primitives build IoRequest values and yield them via SIG_IO.
//! The scheduler catches SIG_IO and passes the request to a backend
//! for execution.

use crate::value::Value;

/// I/O operation descriptor.
#[derive(Debug)]
pub enum IoOp {
    /// Read one line (up to `\n`). Returns string or nil (EOF).
    ReadLine,
    /// Read up to `count` bytes. Returns bytes/string or nil (EOF).
    Read { count: usize },
    /// Read everything remaining. Returns string or bytes.
    ReadAll,
    /// Write data to port. Returns bytes written (int).
    Write { data: Value },
    /// Flush port's write buffer. Returns nil.
    Flush,
}

/// A typed I/O request. Wrapped as ExternalObject with type_name "io-request".
///
/// The port is stored as `Value` (not `&Port`) because:
/// - The `Value` holds the `Rc` to the `ExternalObject` containing the `Port`
/// - The backend extracts `&Port` via `value.as_external::<Port>()`
#[derive(Debug)]
pub struct IoRequest {
    pub op: IoOp,
    pub port: Value,
}

impl IoRequest {
    /// Create an IoRequest Value (ExternalObject with type_name "io-request").
    #[allow(clippy::new_ret_no_self)]
    pub fn new(op: IoOp, port: Value) -> Value {
        Value::external("io-request", IoRequest { op, port })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_request_type_name() {
        let req = IoRequest::new(IoOp::ReadLine, Value::NIL);
        assert_eq!(req.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_io_request_not_port() {
        let req = IoRequest::new(IoOp::Flush, Value::NIL);
        assert_ne!(req.external_type_name(), Some("port"));
    }
}
