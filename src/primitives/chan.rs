//! Channel primitives — crossbeam-channel wrappers for inter-fiber messaging.
//!
//! ## Scheduler-aware select
//!
//! `chan/select` cannot use crossbeam's blocking `Select::select_timeout`
//! because that parks the OS thread on which the fiber scheduler runs —
//! starving any `ev/spawn`'d producer fiber that would have unblocked the
//! select.  Instead each channel carries a shared `WakeList` of eventfd
//! file descriptors.  A selecting fiber allocates an eventfd, registers it
//! in every candidate receiver's `WakeList`, and yields with
//! `IoOp::ChanSelectPark` — the scheduler waits on the eventfd via
//! `IORING_OP_POLL_ADD` (or `poll(2)` on the thread-pool backend), exactly
//! like `ev/poll-fd`.  `chan/send`, after a successful `try_send`, signals
//! every registered eventfd so any parked selector wakes and re-tries.
//! Cross-thread `chan/send` (from `sys/spawn`) wakes the scheduler thread
//! the same way — `write(eventfd, 1)` is thread-safe and the kernel poll
//! notices it.

use crate::primitives::def::RegionEffect;
use std::cell::RefCell;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{self, TryRecvError, TrySendError};

use crate::io::request::{IoOp, IoRequest};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;

mod prims;
use prims::*;

/// Shared wake state between a channel's sender and receiver halves.
///
/// Stores the **write-side** fds of any fibers currently parked in
/// `chan/select` on this channel.  On Linux these are eventfds (poll
/// and wake share one fd); on other Unix these are the write ends of
/// the per-park pipe2 — confusing the two breaks the wake protocol on
/// macOS (the producer would `write(2)` to a pipe's read end).
/// `chan/send` writes a wake byte to each registered fd after a
/// successful `try_send`; `nonempty` is an atomic fast-path so the
/// common case (nobody is selecting) takes no lock.
pub struct WakeList {
    /// Write-side fds.  Iterated under `fds` lock from `wake_all`.
    wake_fds: Mutex<Vec<RawFd>>,
    nonempty: AtomicBool,
    /// The trace cell of the instance that created this channel (a clone of its
    /// heap's). `chan_trace` gates on it, so a `--trace=chan` toggle is scoped to
    /// this channel's own instance — a cross-thread `chan/send` (from `sys/spawn`,
    /// which holds no `&VM`) still reads the right instance's trace because the
    /// `WakeList` travels with the channel.
    trace: crate::config::TraceCell,
}

/// True when this channel's instance has the `chan` trace bit set. Read through
/// the channel's own [`WakeList`]-carried trace cell — per-instance, no
/// process-global, so a cross-thread send still gates on the creating instance.
fn chan_trace_enabled(trace: &crate::config::TraceCell) -> bool {
    trace.load(Ordering::Relaxed) & crate::config::trace_bits::CHAN != 0
}

/// Trace channel wake events (register / deregister / wake_all / write / close)
/// to stderr when the channel's instance has `--trace=chan` set.
///
/// Mirrors `posix_trace` in `io::sigfd`: direct `write(2, …)` syscall
/// to bypass Rust stdio buffering, so trace lines survive even when
/// the process is about to be killed by an outer timeout.
#[inline]
fn chan_trace(trace: &crate::config::TraceCell, args: std::fmt::Arguments<'_>) {
    if !chan_trace_enabled(trace) {
        return;
    }
    let line = format!("[trace:chan] {}\n", args);
    // SAFETY: writing to fd 2 (stderr) is always valid; failures are
    // benign (trace lines are diagnostic, not load-bearing).
    unsafe {
        libc::write(2, line.as_ptr() as *const libc::c_void, line.len());
    }
}

impl WakeList {
    /// Build a wake list carrying `trace` — the creating instance's trace cell
    /// (`ctx.heap().trace_cell()`), so every `chan_trace` this channel emits gates
    /// on that instance's own `--trace=chan` state.
    pub fn new(trace: crate::config::TraceCell) -> Arc<Self> {
        Arc::new(WakeList {
            wake_fds: Mutex::new(Vec::new()),
            nonempty: AtomicBool::new(false),
            trace,
        })
    }

    /// Register a wake fd (the write side of the per-park wake pair —
    /// same as the poll fd only on Linux).
    fn register(&self, wake_fd: RawFd) {
        debug_assert!(wake_fd >= 0, "WakeList::register: invalid fd {}", wake_fd);
        let mut fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        fds.push(wake_fd);
        self.nonempty.store(true, Ordering::Release);
        chan_trace(
            &self.trace,
            format_args!("register fd={} (wake-list len now {})", wake_fd, fds.len()),
        );
    }

    fn deregister(&self, wake_fd: RawFd) {
        debug_assert!(wake_fd >= 0, "WakeList::deregister: invalid fd {}", wake_fd);
        let mut fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        let before = fds.len();
        fds.retain(|&f| f != wake_fd);
        if fds.is_empty() {
            self.nonempty.store(false, Ordering::Release);
        }
        chan_trace(
            &self.trace,
            format_args!(
                "deregister fd={} ({}→{} entries)",
                wake_fd,
                before,
                fds.len()
            ),
        );
    }

    /// Signal every registered wake fd.  Called after a successful
    /// send (or a sender/receiver close) so parked selectors
    /// re-evaluate.  Skipped via the `nonempty` atomic when no one is
    /// selecting on this channel.
    ///
    /// `pub(crate)` so `sys/spawn`'s worker can wake a joiner parked in
    /// `chan/select` on the thread-completion channel (see
    /// `primitives::concurrency`).
    pub(crate) fn wake_all(&self) {
        if !self.nonempty.load(Ordering::Acquire) {
            return;
        }
        let fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        chan_trace(
            &self.trace,
            format_args!("wake_all signaling {} fd(s)", fds.len()),
        );
        for &fd in fds.iter() {
            wake_fd_signal(&self.trace, fd);
        }
    }
}

/// Write a wake byte to a `WakeList` fd.  On Linux the fd is an eventfd
/// (8-byte counter write); on other Unix the fd is the write end of a
/// pipe (single-byte write).  Either way the matching poll on the
/// scheduler thread observes POLLIN and resumes the parked fiber.
#[cfg(target_os = "linux")]
fn wake_fd_signal(trace: &crate::config::TraceCell, fd: RawFd) {
    debug_assert!(fd >= 0, "wake_fd_signal: invalid fd {}", fd);
    // Same 8-byte eventfd write the io backend's bridge uses to wake the
    // scheduler; one definition lives in `crate::io::eventfd`. Failures (EAGAIN
    // on counter overflow, EBADF on already-closed) are benign for the wake
    // protocol — a parked poll either already observed POLLIN or no longer cares.
    let ret = crate::io::eventfd::signal(fd);
    if chan_trace_enabled(trace) {
        let err = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        chan_trace(
            trace,
            format_args!("write(eventfd={}, 1) -> {} errno={}", fd, ret, err),
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn wake_fd_signal(trace: &crate::config::TraceCell, fd: RawFd) {
    debug_assert!(fd >= 0, "wake_fd_signal: invalid fd {}", fd);
    let one: u8 = 1;
    // SAFETY: a single-byte write to a pipe fd is always valid;
    // failures are benign — see Linux variant.
    let ret = unsafe { libc::write(fd, &one as *const u8 as *const libc::c_void, 1) };
    if chan_trace_enabled(trace) {
        let err = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        chan_trace(
            trace,
            format_args!("write(pipe={}, 1) -> {} errno={}", fd, ret, err),
        );
    }
}

/// Allocate a wake fd usable for `IoOp::ChanSelectPark`.
///
/// Returns `(poll_fd, wake_fd)`.  On Linux both are the same eventfd
/// (counter semantics); on other Unix `poll_fd` is the read end and
/// `wake_fd` is the write end of a pipe — they are distinct fds and
/// senders MUST write to `wake_fd`, not `poll_fd`.  Both ends are set
/// `O_NONBLOCK | O_CLOEXEC`.
#[cfg(target_os = "linux")]
fn make_wake_fd(trace: &crate::config::TraceCell) -> std::io::Result<(RawFd, RawFd)> {
    // One non-blocking, close-on-exec eventfd; poll and wake share the one fd on
    // Linux. The same `crate::io::eventfd::create` backs the io backend's bridge.
    let fd = crate::io::eventfd::create()?;
    chan_trace(trace, format_args!("alloc eventfd={}", fd));
    Ok((fd, fd))
}

#[cfg(not(target_os = "linux"))]
fn make_wake_fd(trace: &crate::config::TraceCell) -> std::io::Result<(RawFd, RawFd)> {
    let mut fds: [libc::c_int; 2] = [-1, -1];
    // SAFETY: fds is a 2-element c_int array; pipe(2) writes both
    // entries on success and neither on failure.
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let (read_fd, write_fd) = (fds[0] as RawFd, fds[1] as RawFd);
    assert!(
        read_fd >= 0 && write_fd >= 0,
        "make_wake_fd: pipe(2) returned 0 but produced invalid fds {:?}",
        fds
    );
    // Set O_NONBLOCK + FD_CLOEXEC on both ends.  Failure here would
    // leave us with blocking/inheritable fds, which could deadlock
    // wake_all if a pipe buffer fills.  Treat as a hard error.
    for &fd in &[read_fd, write_fd] {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                let err = std::io::Error::last_os_error();
                libc::close(read_fd);
                libc::close(write_fd);
                return Err(err);
            }
            let cflags = libc::fcntl(fd, libc::F_GETFD);
            if cflags < 0 || libc::fcntl(fd, libc::F_SETFD, cflags | libc::FD_CLOEXEC) < 0 {
                let err = std::io::Error::last_os_error();
                libc::close(read_fd);
                libc::close(write_fd);
                return Err(err);
            }
        }
    }
    chan_trace(
        trace,
        format_args!(
            "alloc pipe poll_fd(read)={} wake_fd(write)={}",
            read_fd, write_fd
        ),
    );
    Ok((read_fd, write_fd))
}

/// RAII guard for one parked `chan/select`.
///
/// Owns the wake-fd pair and a clone of each candidate receiver's
/// `WakeList`.  Constructed in `chan/wait-ready`, transferred into
/// `PendingOp::ChanSelectPark`, and dropped exactly once — on completion,
/// cancellation, or aborted submission.  Drop deregisters from every
/// wake list and closes the fds.
pub struct ChanSelectGuard {
    poll_fd: RawFd,
    wake_fd: RawFd,
    wake_lists: Vec<Arc<WakeList>>,
    /// The selecting instance's trace cell (from `ctx` at `chan/wait-ready`),
    /// so the guard's own wake/close `chan_trace` lines gate per-instance.
    trace: crate::config::TraceCell,
}

impl ChanSelectGuard {
    /// The fd the scheduler should poll for POLLIN.
    pub fn poll_fd(&self) -> RawFd {
        self.poll_fd
    }
}

impl Drop for ChanSelectGuard {
    fn drop(&mut self) {
        debug_assert!(
            self.poll_fd >= 0 && self.wake_fd >= 0,
            "ChanSelectGuard::drop: invalid fds poll={} wake={}",
            self.poll_fd,
            self.wake_fd
        );
        // Deregister our wake fd from every receiver's WakeList first
        // — once deregistered no new sender will signal this fd.
        // Senders that loaded a stale fd just before deregister still
        // race to write to it; the write happens against a fd that
        // may close at any moment.  Both paths (eventfd / pipe write
        // to a closed fd) return EBADF which wake_fd_signal swallows.
        for wl in &self.wake_lists {
            wl.deregister(self.wake_fd);
        }
        // Then wake any in-flight poll so it returns before we close
        // the fd — critical on the thread-pool backend where a worker
        // may still be in libc::poll(2).
        wake_fd_signal(&self.trace, self.wake_fd);
        chan_trace(
            &self.trace,
            format_args!("close poll_fd={} wake_fd={}", self.poll_fd, self.wake_fd),
        );
        // SAFETY: we own both fds; closing twice (same value on Linux)
        // is guarded by a wake_fd == poll_fd check.
        unsafe {
            libc::close(self.poll_fd);
            if self.wake_fd != self.poll_fd {
                libc::close(self.wake_fd);
            }
        }
    }
}

/// Take-once container for the guard inside `IoOp::ChanSelectPark`.
///
/// The submit path takes the guard out and transfers it into the
/// PendingOp; the IoOp's own drop sees `None` and does nothing.  If the
/// IoOp is dropped without ever being submitted (e.g. fiber aborted
/// before the scheduler runs `io/submit`), the guard is still inside the
/// cell and its Drop reclaims the fds and wake-list slots.
pub struct ChanSelectGuardCell(RefCell<Option<ChanSelectGuard>>);

impl ChanSelectGuardCell {
    pub fn new(guard: ChanSelectGuard) -> Self {
        ChanSelectGuardCell(RefCell::new(Some(guard)))
    }

    /// Move the guard out, leaving the cell empty.  Returns None if
    /// already taken (which would indicate a backend bug).
    pub fn take(&self) -> Option<ChanSelectGuard> {
        self.0.borrow_mut().take()
    }
}

impl std::fmt::Debug for ChanSelectGuardCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChanSelectGuardCell(..)")
    }
}

/// Newtype wrapper to satisfy crossbeam's `Send` requirement.
///
/// `Value` contains `Rc` (not `Send`). For single-threaded schedulers
/// (the common case) this is trivially safe. For cross-thread use the
/// scheduler is responsible for only sending immutable data.
pub(crate) struct SendableValue(Value);

impl SendableValue {
    /// Wrap a Value for cross-thread channel transport. Callers must
    /// honor the `Send` contract below — `sys/spawn`'s completion
    /// sentinel is an immediate integer (no heap), which is trivially
    /// safe to move across threads.
    pub(crate) fn new(v: Value) -> Self {
        SendableValue(v)
    }
}

// SAFETY: The scheduler contract guarantees that values sent through
// channels are either immutable or will not be accessed from the
// sending side after the send.
unsafe impl Send for SendableValue {}

/// Sender half of a channel, wrapped for `Value::external`.
///
/// Field 0 is the crossbeam sender (Optional so `chan/close` can drop
/// it without dropping the whole external).  Field 1 is the shared
/// `WakeList` — the same Arc lives in this channel's receiver half so
/// `chan/send` can wake any parked `chan/select`.
pub(crate) struct ChanSender(
    pub(crate) RefCell<Option<crossbeam_channel::Sender<SendableValue>>>,
    pub(crate) Arc<WakeList>,
);

/// Receiver half of a channel, wrapped for `Value::external`.
///
/// Field 1 is the same shared `WakeList` carried by every matching
/// sender — see `ChanSender`.
pub(crate) struct ChanReceiver(
    pub(crate) RefCell<Option<crossbeam_channel::Receiver<SendableValue>>>,
    pub(crate) Arc<WakeList>,
);

/// Clone the crossbeam sender and the shared `WakeList` from a sender
/// Value.  Returns None if the value is not a `chan/sender` or its
/// crossbeam half is already closed.
pub(crate) fn clone_sender(
    v: &Value,
) -> Option<(crossbeam_channel::Sender<SendableValue>, Arc<WakeList>)> {
    let cs = v.as_external::<ChanSender>()?;
    let tx = cs.0.borrow().as_ref().cloned()?;
    Some((tx, Arc::clone(&cs.1)))
}

/// Clone the crossbeam receiver and the shared `WakeList` from a
/// receiver Value.  Returns None if the value is not a `chan/receiver`
/// or its crossbeam half is already closed.
pub(crate) fn clone_receiver(
    v: &Value,
) -> Option<(crossbeam_channel::Receiver<SendableValue>, Arc<WakeList>)> {
    let cr = v.as_external::<ChanReceiver>()?;
    let rx = cr.0.borrow().as_ref().cloned()?;
    Some((rx, Arc::clone(&cr.1)))
}

/// Create a chan/sender Value from a raw crossbeam sender and its
/// shared `WakeList`.  The `WakeList` must be the same Arc that backs
/// the matching receiver(s).
pub(crate) fn sender_value(
    tx: crossbeam_channel::Sender<SendableValue>,
    wake: Arc<WakeList>,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Value {
    ctx.external("chan/sender", ChanSender(RefCell::new(Some(tx)), wake))
}

/// Create a chan/receiver Value from a raw crossbeam receiver and its
/// shared `WakeList`.
pub(crate) fn receiver_value(
    rx: crossbeam_channel::Receiver<SendableValue>,
    wake: Arc<WakeList>,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Value {
    ctx.external("chan/receiver", ChanReceiver(RefCell::new(Some(rx)), wake))
}

primitive! {
    "chan" => prim_chan_new {
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Create a channel. Returns [sender receiver]. Optional capacity for bounded channel.",
        params: &["&opt capacity"],
        category: "chan",
        example: "(chan)",
        aliases: &["chan/new"],
        effect: RegionEffect::Fresh,
    }
    "chan/send" => prim_chan_send {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Non-blocking send. Returns [:ok], [:full], or [:disconnected].",
        params: &["sender", "msg"],
        category: "chan",
        example: "(chan/send sender 42)",
        // `Sends`, not `Stores`: the message (arg 1) crosses to the receiving
        // fiber (by pointer — `prim_chan_send` enqueues the raw `SendableValue`).
        // The store is seam-counted (`retain_sent_message` on a successful
        // enqueue, lowered by `release_received_message` at the receive), so the
        // solver records no edge; the fiber-frontier escape of the message is the
        // escape analysis's fiber/send facet (`hir::escape`), the Shared seed the
        // ownership forest reads.
        effect: RegionEffect::Sends { args: &[1] },
    }
    "chan/recv" => prim_chan_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking receive. Returns [:ok msg], [:empty], or [:disconnected].",
        params: &["receiver"],
        category: "chan",
        example: "(chan/recv receiver)",
        effect: RegionEffect::Fresh,
    }
    "chan/clone" => prim_chan_clone {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Clone a sender. Multiple senders can feed the same channel.",
        params: &["sender"],
        category: "chan",
        example: "(chan/clone sender)",
        effect: RegionEffect::Fresh,
    }
    "chan/close" => prim_chan_close {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a sender. Receivers will get :disconnected after buffered messages drain.",
        params: &["sender"],
        category: "chan",
        example: "(chan/close sender)",
        effect: RegionEffect::Immediate,
    }
    "chan/close-recv" => prim_chan_close_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a receiver. Senders will get :disconnected on next send.",
        params: &["receiver"],
        category: "chan",
        example: "(chan/close-recv receiver)",
        effect: RegionEffect::Immediate,
    }
    "chan/try-select" => prim_chan_try_select {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking poll over receivers. Returns [index msg], [:empty], or [:disconnected].",
        params: &["receivers"],
        category: "chan",
        example: "(chan/try-select @[r1 r2])",
        effect: RegionEffect::Fresh,
    }
    "chan/wait-ready" => prim_chan_wait_ready {
        signal: Signal::io_yields_errors(),
        arity: Arity::Range(1, 2),
        doc: "Park the current fiber until a receiver is ready, a sender closes, or timeout-ms elapses. Returns nil; caller re-checks with chan/try-select.",
        params: &["receivers", "&opt timeout-ms"],
        category: "chan",
        example: "(chan/wait-ready @[r1 r2] 1000)",
        // Fresh: the SIG_OK fast path builds a fresh `[:ready i v]`/`[:disconnected]`
        // array in this call's ctx region (oracle-CHECKED, same reception shape as
        // chan/recv); the yield path (ChanSelectPark) resumes with Value::NIL,
        // which Fresh permits. Stores nothing.
        effect: RegionEffect::Fresh,
    }
}
