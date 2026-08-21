//! PendingOp — in-flight async I/O operation tracking.

use crate::io::pool::BufferHandle;
use crate::io::request::{ConnectAddr, PortOp};
use crate::io::types::PortKey;
use crate::port::PortKind;
use crate::value::Value;
use std::os::unix::io::RawFd;
use std::time::Duration;

/// Pending async I/O operation.
///
/// Three variants matching the three port lifecycles:
/// - `Port`: operates on an existing port (stream I/O, accept, datagram, shutdown)
/// - `Connect`: creates a new port on completion (no existing port)
/// - `Sleep`: portless timer
pub(crate) enum PendingOp {
    /// Operation on an existing port.
    Port {
        op: PortOp,
        port_key: PortKey,
        port: Value,
        /// BufferPool handle for non-read operations. `None` for Read/ReadLine
        /// (which use pre-allocated fiber-heap buffers instead).
        buffer_handle: Option<BufferHandle>,
        /// For Accept: which kind of listener (TcpListener or UnixListener).
        listener_kind: Option<PortKind>,
        /// Bytes of this operation's payload already transferred: read into
        /// the fiber's pre-allocated buffer, or written out to the fd. Both
        /// directions resubmit the remainder from this offset, and the
        /// completion reports `filled + result_code`. Zero for ops that move
        /// no payload.
        filled: usize,
        /// The request's timeout, carried so a resubmission can re-arm the
        /// `LinkTimeout` that bounds it. A payload too large for one syscall
        /// completes over several SQEs, and `:timeout` means "give up after
        /// this long" for each of them rather than for the first alone.
        /// `None` leaves the operation unbounded.
        ///
        /// Only the io_uring backend re-arms a `LinkTimeout`; the thread pool
        /// bounds the op in the worker, so on that platform every submit site
        /// still fills this field and nothing reads it back.
        timeout: Option<Duration>,
    },
    /// Connect to a remote address.
    Connect {
        #[allow(dead_code)]
        addr: ConnectAddr,
        buffer_handle: BufferHandle,
        /// io_uring: pre-created socket fd. Thread pool: set to result fd
        /// on completion. Cleared on connect failure (fd closed).
        connect_fd: Option<RawFd>,
        /// Pre-allocated port Value (born in the solver's region at the call site).
        port: Value,
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
        /// The file path (for error messages).
        path: String,
        buffer_handle: BufferHandle,
        /// Pre-allocated port Value (born in the solver's region at the call site).
        port: Value,
    },
    /// Background task — arbitrary closure running on thread pool.
    Task { buffer_handle: BufferHandle },
    /// DNS resolution via getaddrinfo(3). Portless.
    Resolve { buffer_handle: BufferHandle },
    /// Waiting for filesystem watch events (inotify/kqueue).
    WatchNext {
        watcher: Value,
        buffer_handle: BufferHandle,
    },
    /// Waiting for POSIX signal deliveries (signalfd/kqueue).
    SigNext {
        receiver: Value,
        buffer_handle: BufferHandle,
    },
    /// Poll a raw fd for readiness. Portless.
    PollFd { buffer_handle: BufferHandle },
    /// Park a `chan/wait-ready` selector on its wake fd.  Portless.
    /// The guard owns the wake fd(s) and the wake-list registrations;
    /// dropping this PendingOp (completion, cancellation, or backend
    /// teardown) closes the fd(s) and deregisters automatically.
    ChanSelectPark {
        buffer_handle: BufferHandle,
        #[allow(dead_code)] // kept alive for its Drop side effect
        guard: crate::primitives::chan::ChanSelectGuard,
    },
}

impl PendingOp {
    /// Get the BufferHandle, if any. Returns `None` for read operations
    /// (which use pre-allocated fiber-heap buffers) and `Some(handle)` for
    /// all other operations.
    pub(crate) fn buffer_handle(&self) -> Option<BufferHandle> {
        match self {
            PendingOp::Port { buffer_handle, .. } => *buffer_handle,
            PendingOp::Connect { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Sleep { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ProcessWait { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Open { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Task { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Resolve { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::WatchNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::SigNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::PollFd { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ChanSelectPark { buffer_handle, .. } => Some(*buffer_handle),
        }
    }

    pub(super) fn filled(&self) -> usize {
        match self {
            PendingOp::Port { filled, .. } => *filled,
            _ => 0,
        }
    }

    /// The request's timeout, for a backend re-arming the bound on a
    /// resubmission. `None` for ops that carry no deadline.
    ///
    /// Only `io::uring::drain` calls this, so the allow is narrowed to the
    /// platforms that compile that module out rather than blanket `dead_code`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn timeout(&self) -> Option<Duration> {
        match self {
            PendingOp::Port { timeout, .. } => *timeout,
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(super) fn set_filled(&mut self, val: usize) {
        if let PendingOp::Port { filled, .. } = self {
            *filled = val;
        }
    }
}
