//! IoRequest — typed I/O request descriptors.
//!
//! Stream primitives build IoRequest values and yield them via SIG_IO.
//! The scheduler catches SIG_IO and passes the request to a backend
//! for execution.

use crate::port::{Direction, Encoding, Port};
use crate::value::Value;
use std::cell::RefCell;
use std::process::Child;
use std::time::Duration;

mod spawn;
pub use spawn::*;

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

/// I/O operation descriptor.
#[derive(Debug)]
pub enum IoOp {
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
    /// Seek to a position in a file. Returns new absolute byte offset.
    /// `whence`: libc::SEEK_SET (0), libc::SEEK_CUR (1), libc::SEEK_END (2).
    Seek { offset: i64, whence: i32 },
    /// Query current logical file position (kernel offset minus buffer len).
    /// Returns the logical byte offset as int.
    Tell,
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
    /// Connect to a remote address. Returns connected stream port.
    Connect { addr: ConnectAddr },
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
    /// unchanged — no value is instantiated on the scheduler's heap, so there is
    /// no cross-heap reference (the arena-lifetime "bytes arrive zeroed" bug).
    RecvFrom { count: usize, result: Value },
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

/// Socket options for connect operations.
#[derive(Debug, Default, Clone)]
pub struct SocketOptions {
    pub sndbuf: Option<i32>,
    pub rcvbuf: Option<i32>,
    pub nodelay: Option<bool>,
    pub keepalive: Option<bool>,
}

/// Apply socket options (SO_SNDBUF, SO_RCVBUF, TCP_NODELAY, SO_KEEPALIVE) to a socket fd.
pub(crate) fn apply_socket_options(fd: std::os::unix::io::RawFd, opts: &SocketOptions) {
    unsafe {
        if let Some(val) = opts.sndbuf {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &val as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.rcvbuf {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &val as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.nodelay {
            let opt: i32 = val as i32;
            libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_NODELAY,
                &opt as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
        if let Some(val) = opts.keepalive {
            let opt: i32 = val as i32;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_KEEPALIVE,
                &opt as *const i32 as *const libc::c_void,
                std::mem::size_of::<i32>() as libc::socklen_t,
            );
        }
    }
}

/// Address for connect operations.
///
/// `Tcp.addr` is a **parsed IP** — connect is IP-only at this layer. Hostname
/// resolution lives in the stdlib `tcp/connect` wrapper, which calls `sys/resolve`
/// then the IP-only `tcp/connect-ip` primitive for each returned address. This
/// keeps the backend free of a blocking getaddrinfo fallback: an io_uring connect
/// always has an address it can hand the kernel directly.
#[derive(Debug)]
pub enum ConnectAddr {
    Tcp {
        addr: std::net::IpAddr,
        port: u16,
        options: SocketOptions,
        encoding: crate::port::Encoding,
    },
    Unix {
        path: String,
        options: SocketOptions,
        encoding: crate::port::Encoding,
    },
}

impl ConnectAddr {
    pub fn options(&self) -> &SocketOptions {
        match self {
            ConnectAddr::Tcp { options, .. } => options,
            ConnectAddr::Unix { options, .. } => options,
        }
    }

    pub fn encoding(&self) -> crate::port::Encoding {
        match self {
            ConnectAddr::Tcp { encoding, .. } => *encoding,
            ConnectAddr::Unix { encoding, .. } => *encoding,
        }
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

/// Extract a writeable pointer and length from a pre-allocated LBytes buffer.
///
/// # Safety
///
/// The caller must ensure that:
/// - The fiber that owns this buffer is parked (no mutator can read the data).
/// - The pointer is used only for a single write (kernel SQE or thread pool copy).
/// - The pointer is not used after the fiber is un-parked or torn down.
///
/// The `LBytes` variant stores an `RegionSlice<u8>` pointing into the fiber's
/// bump arena. We cast away `const` to write through it. This is safe because:
///
/// - Bump arena pages are mmap'd with `PROT_READ | PROT_WRITE`.
/// - The fiber is parked — no mutator can observe the write.
/// - The pointer escapes to C (io_uring SQE) — the optimizer cannot assume
///   the pointee is unchanged.
pub(crate) unsafe fn writeable_buffer_ptr(buffer: &Value) -> (*mut u8, usize) {
    use crate::value::heap::HeapObject;
    let ext = buffer.as_heap_ptr().expect("buffer must be heap value");
    let obj = ext as *const HeapObject;
    match &*obj {
        HeapObject::LBytes { data, .. } => (data.as_ptr() as *mut u8, data.len()),
        _ => panic!("IoOp buffer must be LBytes, got {}", buffer.type_name()),
    }
}

/// Truncate a pre-allocated LBytes buffer to the actual number of bytes written.
///
/// The pre-allocated buffer has capacity `N` but only `new_len` bytes are valid.
/// This modifies the RegionSlice's `len` field in place so that `as_bytes()`
/// returns a slice of the correct length.
///
/// # Safety
///
/// Same requirements as `writeable_buffer_ptr`: the owning fiber must be parked.
/// `new_len` must be <= the buffer's current length.
pub(crate) unsafe fn truncate_buffer(buffer: &Value, new_len: usize) {
    use crate::value::heap::HeapObject;
    use crate::value::region_slice::RegionSlice;
    let ext = buffer.as_heap_ptr().expect("buffer must be heap value");
    let obj = ext as *mut HeapObject;
    match &mut *obj {
        HeapObject::LBytes { data, .. } => {
            assert!(
                new_len <= data.len(),
                "truncate_buffer: new_len {} > buffer len {}",
                new_len,
                data.len()
            );
            *data = RegionSlice::from_raw(data.as_ptr(), new_len as u32);
        }
        _ => panic!("IoOp buffer must be LBytes, got {}", buffer.type_name()),
    }
}

/// Transmute a pre-allocated LBytes buffer into an LString in place.
///
/// After `truncate_buffer` has set the correct length, this validates the
/// buffer content as UTF-8 and transmutes the HeapObject from `LBytes` to
/// `LString` without copying data. The returned Value has `TAG_STRING` and
/// points to the same heap allocation.
///
/// This works because `LBytes` and `LString` have identical field layouts:
///
/// ```text
/// LBytes  { data: RegionSlice<u8>, traits: Value }   // HeapTag 22
/// LString { s:    RegionSlice<u8>, traits: Value }   // HeapTag 0
/// ```
///
/// Only the HeapTag discriminant and Value TAG differ — we overwrite both
/// in place via `ptr::write` (which does NOT drop the old value).
///
/// # Safety
///
/// Same requirements as `truncate_buffer`: the owning fiber must be parked.
/// The buffer must be an `LBytes` value (not already transmuted).
///
/// # Returns
///
/// `Ok(Value)` with `TAG_STRING` on valid UTF-8.
/// `Err(Value)` with an encoding error on invalid UTF-8.
pub(crate) unsafe fn bytes_to_string_in_place(
    buffer: Value,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Result<Value, Value> {
    use crate::value::heap::HeapObject;
    use crate::value::region_slice::RegionSlice;
    use crate::value::repr::TAG_STRING;

    let ptr = buffer.as_heap_ptr().expect("buffer must be heap value") as *mut HeapObject;

    let (slice_ptr, slice_len, traits) = match &*ptr {
        HeapObject::LBytes { data, traits } => (data.as_ptr(), data.len(), *traits),
        _ => panic!(
            "bytes_to_string_in_place: expected LBytes, got {}",
            buffer.type_name()
        ),
    };

    // Validate UTF-8
    let bytes = std::slice::from_raw_parts(slice_ptr, slice_len);
    if std::str::from_utf8(bytes).is_err() {
        // The error value is built on the requesting instance's heap, threaded
        // in by the caller (the same `origin_heap` every io completion uses).
        return Err(crate::io::io_error(
            "encoding-error",
            format!("port/read-line: invalid UTF-8 in {} bytes", slice_len),
            origin_heap,
        ));
    }

    // Transmute: overwrite HeapObject in place (LBytes → LString).
    // ptr::write does NOT drop the old value — safe because neither
    // RegionSlice<u8> nor Value has a Drop impl.
    std::ptr::write(
        ptr,
        HeapObject::LString {
            s: RegionSlice::from_raw(slice_ptr, slice_len as u32),
            traits,
        },
    );

    // Return new Value with TAG_STRING, same heap pointer
    Ok(Value::from_heap_ptr(ptr as *const (), TAG_STRING))
}

/// Overwrite one field of a pre-allocated **immutable** struct in place.
///
/// Used by the `RecvFrom` completion to stamp the kernel-derived `:addr`
/// (an `LString` re-tagged from the pre-allocated `:addr` buffer) and `:port`
/// (an int) into the result struct that was pre-allocated on the requesting
/// fiber's heap — without re-composing the struct or allocating a new one.
///
/// `LStruct.data` is a plain `Vec<(TableKey, Value)>` (on the Rust heap, not the
/// arena), so this is an ordinary in-place slot write; the sorted key order is
/// preserved because we never change a key.
///
/// # Safety
///
/// Same contract as the other in-place fill helpers: the owning fiber must be
/// parked (exclusive access). `key` must already be present. The replaced and
/// replacement values must not require RC fixups beyond what the caller manages
/// — in practice the old slot holds a placeholder (`int 0` / pre-alloc buffer)
/// and the new value is owned by / lives on the same fiber heap.
pub(crate) unsafe fn set_struct_field_in_place(
    result: &Value,
    key: &crate::value::heap::TableKey,
    val: Value,
) {
    use crate::value::heap::HeapObject;
    let ptr = result
        .as_heap_ptr()
        .expect("recv result must be a heap value") as *mut HeapObject;
    match &mut *ptr {
        HeapObject::LStruct { data, .. } => {
            for entry in data.iter_mut() {
                if &entry.0 == key {
                    entry.1 = val;
                    return;
                }
            }
            panic!("set_struct_field_in_place: key {:?} not present", key);
        }
        _ => panic!(
            "set_struct_field_in_place: expected immutable struct, got {}",
            result.type_name()
        ),
    }
}

#[cfg(test)]
mod tests;
