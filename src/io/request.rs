//! IoRequest — typed I/O request descriptors.
//!
//! Stream primitives build IoRequest values and yield them via SIG_IO.
//! The scheduler catches SIG_IO and passes the request to a backend
//! for execution.

use crate::value::Value;
use std::time::Duration;

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
    /// Accept a connection on a listener. Returns new stream port.
    Accept,
    /// Connect to a remote address. Returns connected stream port.
    Connect { addr: ConnectAddr },
    /// Send data to a remote address via UDP. Returns bytes sent.
    SendTo {
        addr: String,
        port_num: u16,
        data: Value,
    },
    /// Receive data from a UDP socket. Returns (data, remote_addr).
    RecvFrom { count: usize },
    /// Shutdown a socket connection. Returns nil.
    Shutdown { how: i32 },
}

/// Address for connect operations.
#[derive(Debug)]
pub enum ConnectAddr {
    Tcp { addr: String, port: u16 },
    Unix { path: String },
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
    pub timeout: Option<Duration>,
}

impl IoRequest {
    /// Create an IoRequest Value (ExternalObject with type_name "io-request").
    #[allow(clippy::new_ret_no_self)]
    pub fn new(op: IoOp, port: Value) -> Value {
        Value::external(
            "io-request",
            IoRequest {
                op,
                port,
                timeout: None,
            },
        )
    }

    /// Create an IoRequest with a timeout.
    #[allow(clippy::new_ret_no_self)]
    pub fn with_timeout(op: IoOp, port: Value, timeout: Option<Duration>) -> Value {
        Value::external("io-request", IoRequest { op, port, timeout })
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

    #[test]
    fn test_io_request_with_timeout() {
        let timeout = Some(Duration::from_millis(5000));
        let req = IoRequest::with_timeout(IoOp::ReadLine, Value::NIL, timeout);
        let extracted = req.as_external::<IoRequest>().unwrap();
        assert_eq!(extracted.timeout, timeout);
    }
}
