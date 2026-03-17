//! PendingOp — in-flight async I/O operation tracking.

use crate::io::pool::BufferHandle;
use crate::io::request::{ConnectAddr, IoOp};
use crate::io::types::PortKey;
use crate::port::{Direction, Encoding, PortKind};
use crate::value::Value;
use std::os::unix::io::RawFd;

/// Pending async I/O operation.
///
/// Three variants matching the three port lifecycles:
/// - `Port`: operates on an existing port (stream I/O, accept, datagram, shutdown)
/// - `Connect`: creates a new port on completion (no existing port)
/// - `Sleep`: portless timer
pub(crate) enum PendingOp {
    /// Operation on an existing port.
    Port {
        op: IoOp,
        port_key: PortKey,
        port: Value,
        buffer_handle: BufferHandle,
        /// For Accept: which kind of listener (TcpListener or UnixListener).
        listener_kind: Option<PortKind>,
    },
    /// Connect to a remote address. Creates a new port on completion.
    Connect {
        addr: ConnectAddr,
        buffer_handle: BufferHandle,
        /// io_uring: pre-created socket fd. Thread pool: set to result fd
        /// on completion. Cleared on connect failure (fd closed).
        connect_fd: Option<RawFd>,
    },
    /// Async timer. No port.
    Sleep { buffer_handle: BufferHandle },
    /// Waiting for subprocess exit via IORING_OP_WAITID.
    ///
    /// SAFETY: `siginfo` is a heap-allocated `siginfo_t` (via Box::into_raw).
    /// It must live until the CQE arrives. Released in completion processing.
    ProcessWait {
        buffer_handle: BufferHandle,
        handle_val: Value, // ProcessHandle — to cache exit code on completion
        siginfo: *mut libc::siginfo_t, // kernel fills this when child exits
    },
    /// Open a file path. Creates a new port on completion.
    ///
    /// For io_uring: the null-terminated path bytes are stored in the buffer
    /// pool slot (via buffer_handle) so they stay pinned until the CQE arrives.
    /// For thread pool: path is owned by the PoolOp::Open; buffer_handle is a
    /// dummy allocation (0 bytes).
    Open {
        /// The file path (for error messages and Port construction).
        path: String,
        direction: Direction,
        encoding: Encoding,
        buffer_handle: BufferHandle,
    },
}

impl PendingOp {
    pub(crate) fn buffer_handle(&self) -> BufferHandle {
        match self {
            PendingOp::Port { buffer_handle, .. } => *buffer_handle,
            PendingOp::Connect { buffer_handle, .. } => *buffer_handle,
            PendingOp::Sleep { buffer_handle, .. } => *buffer_handle,
            PendingOp::ProcessWait { buffer_handle, .. } => *buffer_handle,
            PendingOp::Open { buffer_handle, .. } => *buffer_handle,
        }
    }
}
