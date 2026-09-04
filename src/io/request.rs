//! IoRequest — typed I/O request descriptors.
//!
//! Stream primitives build IoRequest values and yield them via SIG_IO.
//! The scheduler catches SIG_IO and passes the request to a backend
//! for execution.

use crate::port::{Direction, Encoding, Port};
use crate::value::Value;
use std::cell::RefCell;
use std::time::Duration;

mod buffer;
mod process;
mod socket;
mod spawn;

pub use socket::*;
pub use spawn::*;
// In-place buffer fill helpers and the process handle keep their original
// crate-internal visibility; re-export so `crate::io::request::<Item>` paths
// resolve unchanged.
pub(crate) use buffer::{
    bytes_to_string_in_place, set_struct_field_in_place, truncate_buffer, writeable_buffer_ptr,
};
pub(crate) use process::{exit_code_from_siginfo, ExitRecord, ProcessHandle, Reap};
#[cfg(test)]
pub(crate) use process::{reaped_child, zombie_child};

/// Boxed closure type for `IoOp::Task`.
pub type TaskClosure = Box<dyn FnOnce() -> (i32, Vec<u8>) + Send>;

/// A take-once closure for `IoOp::Task`.
///
/// Wraps a `FnOnce` in `RefCell<Option<...>>` so it can be moved out of a
/// shared `&IoRequest` reference. The closure runs on a background thread
/// (async backend) or inline (sync backend) and returns `(i32, Vec<u8>)`:
/// non-negative result_code = success (data returned as bytes),
/// negative result_code = error (data is UTF-8 error message).
pub struct TaskFn {
    inner: RefCell<Option<TaskClosure>>,
}

impl TaskFn {
    pub fn new(f: TaskClosure) -> Self {
        TaskFn {
            inner: RefCell::new(Some(f)),
        }
    }

    /// Take the closure out. Returns `None` if already taken.
    pub(crate) fn take(&self) -> Option<TaskClosure> {
        self.inner.borrow_mut().take()
    }
}

impl std::fmt::Debug for TaskFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let taken = self.inner.borrow().is_none();
        if taken {
            write!(f, "TaskFn(<taken>)")
        } else {
            write!(f, "TaskFn(..)")
        }
    }
}

/// An operation on an already-open port that a backend runs asynchronously, so
/// it outlives its submission as a `PendingOp::Port` entry until a completion
/// arrives.
///
/// Every field is clonable, which is what lets the submit path copy a request's
/// op into that entry. The whole enum is the set the completion path can see:
/// a variant here is one the backend must be able to finish.
///
/// `Close`, `Seek` and `Tell` also address a port but finish inside
/// `AsyncBackend::submit` without reaching a backend, so they stay on [`IoOp`].
#[derive(Debug, Clone)]
pub enum PortOp {
    /// Read one line (up to `\n`). Returns bytes or nil (EOF).
    /// The buffer is pre-allocated on the fiber's heap.
    ReadLine {
        /// Pre-allocated LBytes buffer on the fiber's heap (64KB).
        buffer: Value,
    },
    /// Read up to `count` bytes. Returns bytes or nil (EOF).
    /// The buffer is pre-allocated on the fiber's heap.
    Read {
        count: usize,
        /// Pre-allocated LBytes buffer on the fiber's heap.
        buffer: Value,
    },
    /// Read exactly `count` bytes, looping over short reads. Returns
    /// bytes/string of length `count`, or nil if the stream ended
    /// before `count` bytes arrived. Unlike `Read`, this resubmits
    /// short reads on stream sockets too — `Read` follows POSIX "up
    /// to N" semantics on streams; this is the "no, really, exactly
    /// N" variant for length-prefixed framing.
    /// The buffer is pre-allocated on the fiber's heap.
    ReadExact {
        count: usize,
        /// Pre-allocated LBytes buffer on the fiber's heap (`count` bytes).
        buffer: Value,
    },
    /// Read everything remaining. Returns bytes.
    /// No pre-allocated buffer — unbounded, uses fd_states.buffer accumulation.
    ReadAll,
    /// Write data to port. Returns bytes written (int).
    Write { data: Value },
    /// Flush port's write buffer. Returns nil.
    Flush,
    /// Accept a connection on a listener. Returns new stream port.
    /// Socket options are applied to the accepted fd after accept(2).
    /// `encoding` controls the resulting port's text/binary mode —
    /// callers default to Binary (POSIX sockets are byte streams);
    /// pass Text for line-oriented protocols (SMTP, IRC, etc.).
    /// `accept_port` is pre-allocated at the call site (solver's region).
    Accept {
        options: SocketOptions,
        encoding: crate::port::Encoding,
        accept_port: Value,
    },
    /// Send data to a remote address via UDP. Returns bytes sent.
    SendTo {
        addr: String,
        port_num: u16,
        data: Value,
    },
    /// Receive data from a UDP socket. Returns a struct `{:data :addr :port}`.
    ///
    /// `result` is a pre-allocated immutable `{:data <LBytes(count)> :addr
    /// <LBytes(INET6_ADDRSTRLEN)> :port 0}` struct born on the **requesting
    /// fiber's heap** (like `Read`'s `buffer`). The kernel writes the datagram
    /// payload directly into `:data` (zero-copy via the iovec), and the
    /// completion fills `:data`/`:addr`/`:port` in place and returns `result`
    /// unchanged. Nothing is instantiated on the scheduler's heap, so the
    /// resumed fiber holds no cross-heap reference.
    RecvFrom { count: usize, result: Value },
    /// Shutdown a socket connection. Returns nil.
    Shutdown { how: i32 },
}

/// I/O operation descriptor.
#[derive(Debug)]
pub enum IoOp {
    /// An asynchronous operation on an already-open port. These are the only
    /// ops that become an in-flight `PendingOp::Port` entry.
    Port(PortOp),
    /// Seek to a position in a file. Returns new absolute byte offset.
    /// `whence`: libc::SEEK_SET (0), libc::SEEK_CUR (1), libc::SEEK_END (2).
    Seek { offset: i64, whence: i32 },
    /// Query current logical file position (kernel offset minus buffer len).
    /// Returns the logical byte offset as int.
    Tell,
    /// Connect to a remote address. Returns connected stream port.
    Connect { addr: ConnectAddr },
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
    /// Run an arbitrary closure on a background thread.
    /// Returns bytes on success, error on failure.
    #[allow(dead_code)]
    Task(TaskFn),
    /// Resolve a hostname to IP addresses via getaddrinfo(3).
    /// Portless — always dispatched to the thread pool.
    /// Returns an array of IP address strings.
    Resolve { hostname: String },
    /// Wait for filesystem events from an FsWatcher (inotify/kqueue).
    /// Portless — the FsWatcher External is in the IoRequest.port field.
    WatchNext,
    /// Wait for POSIX signal deliveries from a SignalReceiver
    /// (signalfd on Linux, kqueue+EVFILT_SIGNAL on macOS).
    /// Portless — the SignalReceiver External is in IoRequest.port.
    SigNext,
    /// Close a port: cancel pending I/O ops on its fd, then close the fd.
    /// The scheduler handles the cancel-then-close sequence so that
    /// io_uring operations are properly cancelled before the fd is dropped.
    Close,
    /// Poll a raw fd for readiness. Portless — no existing port.
    /// Uses `IORING_OP_POLL_ADD` on io_uring, `libc::poll()` on thread pool.
    /// Returns revents mask (int) on completion.
    PollFd {
        fd: std::os::unix::io::RawFd,
        events: u32,
    },
    /// Park a fiber on a `chan/wait-ready` wake fd until any sender
    /// signals it or `timeout` (carried in `IoRequest.timeout`) elapses.
    /// Portless.  The guard owns the eventfd / pipe2 fds and the Arc
    /// clones of each receiver's `WakeList`; its Drop deregisters and
    /// closes when the op completes, is cancelled, or never makes it
    /// past submit.
    ChanSelectPark(crate::primitives::chan::ChanSelectGuardCell),
}

impl From<PortOp> for IoOp {
    fn from(op: PortOp) -> Self {
        IoOp::Port(op)
    }
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
    /// Create an IoRequest Value (ExternalObject "io-request"), born in `ctx`'s
    /// region — the requesting native call's own region on its instance heap. It
    /// escapes to the io backend as the primitive's yielded result, freed
    /// value-based.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ctx: &crate::primitives::ctx::Alloc, op: IoOp, port: Value) -> Value {
        ctx.external(
            "io-request",
            IoRequest {
                op,
                port,
                timeout: None,
            },
        )
    }

    /// Create an IoRequest with a timeout, born in `ctx`'s region.
    #[allow(clippy::new_ret_no_self)]
    pub fn with_timeout(
        ctx: &crate::primitives::ctx::Alloc,
        op: IoOp,
        port: Value,
        timeout: Option<Duration>,
    ) -> Value {
        ctx.external("io-request", IoRequest { op, port, timeout })
    }

    /// Create a portless IoRequest (e.g., Sleep), born in `ctx`'s region.
    #[allow(clippy::new_ret_no_self)]
    pub fn portless(ctx: &crate::primitives::ctx::Alloc, op: IoOp) -> Value {
        ctx.external(
            "io-request",
            IoRequest {
                op,
                port: Value::NIL,
                timeout: None,
            },
        )
    }

    /// Create a Task IoRequest — runs a closure on a background thread.
    ///
    /// The closure returns `(i32, Vec<u8>)`:
    /// - Non-negative result_code: success, data returned as `Value::bytes`
    /// - Negative result_code: error, data is UTF-8 error message
    ///
    /// Async backend: closure runs on the thread pool, fiber yields until done.
    /// Sync backend: closure runs inline (blocking).
    #[allow(clippy::new_ret_no_self, dead_code)]
    pub fn task(
        ctx: &crate::primitives::ctx::Alloc,
        f: impl FnOnce() -> (i32, Vec<u8>) + Send + 'static,
    ) -> Value {
        Self::portless(ctx, IoOp::Task(TaskFn::new(Box::new(f))))
    }

    /// Poll a raw fd for readiness. Portless.
    ///
    /// Async backend: uses `IORING_OP_POLL_ADD` or `libc::poll()` on thread pool.
    /// Returns revents mask as int on completion.
    #[allow(clippy::new_ret_no_self)]
    pub fn poll_fd(
        ctx: &crate::primitives::ctx::Alloc,
        fd: std::os::unix::io::RawFd,
        events: u32,
    ) -> Value {
        Self::portless(ctx, IoOp::PollFd { fd, events })
    }

    /// Poll a raw fd with a timeout, born in `ctx`'s region.
    #[allow(clippy::new_ret_no_self)]
    pub fn poll_fd_with_timeout(
        ctx: &crate::primitives::ctx::Alloc,
        fd: std::os::unix::io::RawFd,
        events: u32,
        timeout: Duration,
    ) -> Value {
        ctx.external(
            "io-request",
            IoRequest {
                op: IoOp::PollFd { fd, events },
                port: Value::NIL,
                timeout: Some(timeout),
            },
        )
    }
}

#[cfg(test)]
mod tests;
