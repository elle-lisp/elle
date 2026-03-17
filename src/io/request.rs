//! IoRequest — typed I/O request descriptors.
//!
//! Stream primitives build IoRequest values and yield them via SIG_IO.
//! The scheduler catches SIG_IO and passes the request to a backend
//! for execution.

use crate::port::{Direction, Encoding};
use crate::value::Value;
use std::cell::RefCell;
use std::process::Child;
use std::time::Duration;

/// How to configure a subprocess stdio stream.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StdioDisposition {
    /// Create a pipe; return it as a port.
    Pipe,
    /// Inherit the parent process fd.
    Inherit,
    /// Redirect to /dev/null.
    Null,
}

/// Subprocess configuration, shared between IoOp::Spawn and the backend helpers.
#[derive(Debug)]
pub(crate) struct SpawnRequest {
    pub program: String,
    pub args: Vec<String>,
    pub env: Option<Vec<(String, String)>>,
    pub cwd: Option<String>,
    pub stdin: StdioDisposition,
    pub stdout: StdioDisposition,
    pub stderr: StdioDisposition,
}

/// I/O operation descriptor.
#[derive(Debug)]
pub(crate) enum IoOp {
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
    /// Async sleep. No port — just a timer. Returns nil after duration elapses.
    Sleep { duration: Duration },
    /// Spawn a subprocess. Returns a struct:
    /// {:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <external:process>}
    Spawn(SpawnRequest),
    /// Wait for a subprocess to exit. Returns exit code (int).
    /// The request.port field carries the ProcessHandle value.
    ProcessWait,
    /// Open a file. Returns a port on completion.
    /// No existing port — the port is created on completion.
    Open {
        path: String,
        /// POSIX open(2) flags: O_RDONLY, O_WRONLY|O_CREAT|O_TRUNC, etc.
        /// O_CLOEXEC is always included.
        flags: i32,
        /// File creation mode (permissions). Standard value: 0o666 (umask applied).
        mode: u32,
        direction: Direction,
        encoding: Encoding,
    },
}

/// Address for connect operations.
#[derive(Debug)]
pub(crate) enum ConnectAddr {
    Tcp { addr: String, port: u16 },
    Unix { path: String },
}

/// A typed I/O request. Wrapped as ExternalObject with type_name "io-request".
///
/// The port is stored as `Value` (not `&Port`) because:
/// - The `Value` holds the `Rc` to the `ExternalObject` containing the `Port`
/// - The backend extracts `&Port` via `value.as_external::<Port>()`
#[derive(Debug)]
pub(crate) struct IoRequest {
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

    /// Create a portless IoRequest (e.g., Sleep).
    #[allow(clippy::new_ret_no_self)]
    pub fn portless(op: IoOp) -> Value {
        Value::external(
            "io-request",
            IoRequest {
                op,
                port: Value::NIL,
                timeout: None,
            },
        )
    }
}

/// Handle to a running subprocess. Stored as ExternalObject with type_name "process".
#[derive(Debug)]
pub(crate) struct ProcessHandle {
    pid: u32,
    pub(crate) inner: RefCell<ProcessState>,
}

/// Lifecycle state of a subprocess.
#[derive(Debug)]
pub(crate) enum ProcessState {
    Running(Child),
    Exited(i32), // cached exit code
}

impl ProcessHandle {
    pub fn new(pid: u32, child: Child) -> Self {
        ProcessHandle {
            pid,
            inner: RefCell::new(ProcessState::Running(child)),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

/// Reap the subprocess on drop to prevent zombie accumulation.
/// `try_wait` is non-blocking; if the process hasn't exited yet,
/// it stays in the OS process table until it does.
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if let ProcessState::Running(ref mut child) = *self.inner.borrow_mut() {
            let _ = child.try_wait();
        }
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

    #[test]
    fn test_stdio_disposition_derives() {
        // Smoke test that StdioDisposition is Copy + Clone + Debug
        let d = StdioDisposition::Pipe;
        let _ = d; // Copy
        let _ = format!("{:?}", d); // Debug
    }

    #[test]
    fn test_process_handle_pid() {
        // Spawn /bin/true, verify pid() returns a nonzero value.
        // This test requires /bin/true to exist.
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        assert_eq!(handle.pid(), pid);
        assert!(handle.pid() > 0);
    }

    #[test]
    fn test_process_handle_drop_does_not_panic() {
        // Drop with a running child should not panic.
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        drop(handle); // should not panic
    }
}
