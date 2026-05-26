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
use crate::value::{error_val, Value};

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
}

/// Trace channel wake events (register / deregister / wake_all /
/// write / close) to stderr when `--trace=chan` is set.  Gated on the
/// process-global trace bits so threadpool worker threads and other
/// off-VM sites (which have no `&VM` reference) gate cheaply.
///
/// Mirrors `posix_trace` in `io::sigfd`: direct `write(2, …)` syscall
/// to bypass Rust stdio buffering, so trace lines survive even when
/// the process is about to be killed by an outer timeout.
fn chan_trace_enabled() -> bool {
    crate::config::global_trace_bit_enabled(crate::config::trace_bits::CHAN)
}

#[inline]
fn chan_trace(args: std::fmt::Arguments<'_>) {
    if !chan_trace_enabled() {
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
    pub fn new() -> Arc<Self> {
        Arc::new(WakeList {
            wake_fds: Mutex::new(Vec::new()),
            nonempty: AtomicBool::new(false),
        })
    }

    /// Register a wake fd (the write side of the per-park wake pair —
    /// same as the poll fd only on Linux).
    fn register(&self, wake_fd: RawFd) {
        debug_assert!(wake_fd >= 0, "WakeList::register: invalid fd {}", wake_fd);
        let mut fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        fds.push(wake_fd);
        self.nonempty.store(true, Ordering::Release);
        chan_trace(format_args!(
            "register fd={} (wake-list len now {})",
            wake_fd,
            fds.len()
        ));
    }

    fn deregister(&self, wake_fd: RawFd) {
        debug_assert!(wake_fd >= 0, "WakeList::deregister: invalid fd {}", wake_fd);
        let mut fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        let before = fds.len();
        fds.retain(|&f| f != wake_fd);
        if fds.is_empty() {
            self.nonempty.store(false, Ordering::Release);
        }
        chan_trace(format_args!(
            "deregister fd={} ({}→{} entries)",
            wake_fd,
            before,
            fds.len()
        ));
    }

    /// Signal every registered wake fd.  Called after a successful
    /// send (or a sender/receiver close) so parked selectors
    /// re-evaluate.  Skipped via the `nonempty` atomic when no one is
    /// selecting on this channel.
    fn wake_all(&self) {
        if !self.nonempty.load(Ordering::Acquire) {
            return;
        }
        let fds = self.wake_fds.lock().expect("WakeList lock poisoned");
        chan_trace(format_args!("wake_all signaling {} fd(s)", fds.len()));
        for &fd in fds.iter() {
            wake_fd_signal(fd);
        }
    }
}

/// Write a wake byte to a `WakeList` fd.  On Linux the fd is an eventfd
/// (8-byte counter write); on other Unix the fd is the write end of a
/// pipe (single-byte write).  Either way the matching poll on the
/// scheduler thread observes POLLIN and resumes the parked fiber.
#[cfg(target_os = "linux")]
fn wake_fd_signal(fd: RawFd) {
    debug_assert!(fd >= 0, "wake_fd_signal: invalid fd {}", fd);
    let one: u64 = 1;
    // SAFETY: writing 8 bytes to an eventfd is always valid; failures
    // (EAGAIN if counter would overflow, EBADF on already-closed) are
    // benign for the wake protocol — a parked poll either already
    // observed POLLIN or no longer cares.
    let ret = unsafe {
        libc::write(
            fd,
            &one as *const u64 as *const libc::c_void,
            std::mem::size_of::<u64>(),
        )
    };
    if chan_trace_enabled() {
        let err = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        chan_trace(format_args!(
            "write(eventfd={}, 1) -> {} errno={}",
            fd, ret, err
        ));
    }
}

#[cfg(not(target_os = "linux"))]
fn wake_fd_signal(fd: RawFd) {
    debug_assert!(fd >= 0, "wake_fd_signal: invalid fd {}", fd);
    let one: u8 = 1;
    // SAFETY: a single-byte write to a pipe fd is always valid;
    // failures are benign — see Linux variant.
    let ret = unsafe { libc::write(fd, &one as *const u8 as *const libc::c_void, 1) };
    if chan_trace_enabled() {
        let err = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        chan_trace(format_args!(
            "write(pipe={}, 1) -> {} errno={}",
            fd, ret, err
        ));
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
fn make_wake_fd() -> std::io::Result<(RawFd, RawFd)> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    chan_trace(format_args!("alloc eventfd={}", fd));
    Ok((fd, fd))
}

#[cfg(not(target_os = "linux"))]
fn make_wake_fd() -> std::io::Result<(RawFd, RawFd)> {
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
    chan_trace(format_args!(
        "alloc pipe poll_fd(read)={} wake_fd(write)={}",
        read_fd, write_fd
    ));
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
        wake_fd_signal(self.wake_fd);
        chan_trace(format_args!(
            "close poll_fd={} wake_fd={}",
            self.poll_fd, self.wake_fd
        ));
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
) -> Value {
    Value::external("chan/sender", ChanSender(RefCell::new(Some(tx)), wake))
}

/// Create a chan/receiver Value from a raw crossbeam receiver and its
/// shared `WakeList`.
pub(crate) fn receiver_value(
    rx: crossbeam_channel::Receiver<SendableValue>,
    wake: Arc<WakeList>,
) -> Value {
    Value::external("chan/receiver", ChanReceiver(RefCell::new(Some(rx)), wake))
}

/// Helper: extract `&ChanSender` from a Value or return a type error.
fn extract_sender<'a>(
    value: &'a Value,
    prim_name: &str,
) -> Result<&'a ChanSender, (SignalBits, Value)> {
    value.as_external::<ChanSender>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected chan/sender, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// Helper: extract `&ChanReceiver` from a Value or return a type error.
fn extract_receiver<'a>(
    value: &'a Value,
    prim_name: &str,
) -> Result<&'a ChanReceiver, (SignalBits, Value)> {
    value.as_external::<ChanReceiver>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected chan/receiver, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// Validate that `arg` is a non-empty array whose every element is a
/// `chan/receiver` Value, then invoke `f` with a slice of refs to each
/// underlying `ChanReceiver`.  Errors short-circuit out; otherwise the
/// closure's result is returned.
///
/// Both `chan/try-select` and `chan/wait-ready`'s post-register re-check
/// need the same validation + receiver-slice prep, but each then
/// borrows the inner `Option<Receiver>` cells slightly differently
/// (try-select errors on closed, wait-ready falls through to yield).
/// Passing a closure keeps the borrow lifetimes self-contained.
fn with_receivers<R>(
    arg: &Value,
    op_name: &str,
    f: impl FnOnce(&[&ChanReceiver]) -> R,
) -> Result<R, (SignalBits, Value)> {
    let cell = arg.as_array_mut().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected array of receivers, got {}",
                    op_name,
                    arg.type_name()
                ),
            ),
        )
    })?;
    let vec = cell.borrow();
    if vec.is_empty() {
        return Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!("{}: receivers array is empty", op_name),
            ),
        ));
    }
    let mut recvs: Vec<&ChanReceiver> = Vec::with_capacity(vec.len());
    for (i, val) in vec.iter().enumerate() {
        let cr = val.as_external::<ChanReceiver>().ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: element {} is not a chan/receiver, got {}",
                        op_name,
                        i,
                        val.external_type_name().unwrap_or(val.type_name())
                    ),
                ),
            )
        })?;
        recvs.push(cr);
    }
    Ok(f(&recvs))
}

/// `(chan)` or `(chan capacity)`
///
/// Returns `[sender receiver]` as an array.
fn prim_chan_new(args: &[Value]) -> (SignalBits, Value) {
    let (tx, rx) = if args.is_empty() {
        crossbeam_channel::unbounded()
    } else {
        let cap = match args[0].as_int() {
            Some(n) if n >= 0 => n as usize,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("chan: capacity must be non-negative, got {}", n),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "chan: expected integer for capacity, got {}",
                            args[0].type_name()
                        ),
                    ),
                );
            }
        };
        crossbeam_channel::bounded(cap)
    };

    let wake = WakeList::new();
    let sender = Value::external(
        "chan/sender",
        ChanSender(RefCell::new(Some(tx)), Arc::clone(&wake)),
    );
    let receiver = Value::external("chan/receiver", ChanReceiver(RefCell::new(Some(rx)), wake));
    (SIG_OK, Value::array(vec![sender, receiver]))
}

/// `(chan/send sender msg)` — non-blocking send.
///
/// Returns `[:ok]`, `[:full]`, or `[:disconnected]`.
fn prim_chan_send(args: &[Value]) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/send") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    let tx = match inner.as_ref() {
        Some(tx) => tx,
        None => return (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
    };

    let result = tx.try_send(SendableValue(args[1]));
    match result {
        Ok(()) => {
            sender.1.wake_all();
            (SIG_OK, Value::array(vec![Value::keyword("ok")]))
        }
        Err(TrySendError::Full(_)) => (SIG_OK, Value::array(vec![Value::keyword("full")])),
        Err(TrySendError::Disconnected(_)) => {
            (SIG_OK, Value::array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/recv receiver)` — non-blocking receive.
///
/// Returns `[:ok msg]`, `[:empty]`, or `[:disconnected]`.
fn prim_chan_recv(args: &[Value]) -> (SignalBits, Value) {
    let receiver = match extract_receiver(&args[0], "chan/recv") {
        Ok(r) => r,
        Err(e) => return e,
    };

    let inner = receiver.0.borrow();
    let rx = match inner.as_ref() {
        Some(rx) => rx,
        None => return (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
    };

    match rx.try_recv() {
        Ok(SendableValue(v)) => (SIG_OK, Value::array(vec![Value::keyword("ok"), v])),
        Err(TryRecvError::Empty) => (SIG_OK, Value::array(vec![Value::keyword("empty")])),
        Err(TryRecvError::Disconnected) => {
            (SIG_OK, Value::array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/clone sender)` — clone the sender half.
fn prim_chan_clone(args: &[Value]) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/clone") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    match inner.as_ref() {
        Some(tx) => {
            let cloned = tx.clone();
            (
                SIG_OK,
                Value::external(
                    "chan/sender",
                    ChanSender(RefCell::new(Some(cloned)), Arc::clone(&sender.1)),
                ),
            )
        }
        None => (
            SIG_ERROR,
            error_val("state-error", "chan/clone: sender is closed"),
        ),
    }
}

/// `(chan/close sender)` — close the sender half.
///
/// Drops the inner `Sender`, disconnecting the channel from this end.
/// Wakes any parked `chan/select` so it observes `[:disconnected]` once
/// every sender clone is gone (crossbeam reports the channel as
/// disconnected only after the last sender drops).
fn prim_chan_close(args: &[Value]) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/close") {
        Ok(s) => s,
        Err(e) => return e,
    };

    sender.0.borrow_mut().take();
    sender.1.wake_all();
    (SIG_OK, Value::NIL)
}

/// `(chan/close-recv receiver)` — close the receiver half.
///
/// Drops the inner `Receiver`, disconnecting the channel from this end.
/// Wakes any parked `chan/select` so it observes `[:disconnected]`.
fn prim_chan_close_recv(args: &[Value]) -> (SignalBits, Value) {
    let receiver = match extract_receiver(&args[0], "chan/close-recv") {
        Ok(r) => r,
        Err(e) => return e,
    };

    receiver.0.borrow_mut().take();
    receiver.1.wake_all();
    (SIG_OK, Value::NIL)
}

/// `(chan/try-select receivers)` — non-blocking poll over receivers.
///
/// Returns `[index msg]` if some receiver has a value ready right now,
/// `[:empty]` if none are ready, or `[:disconnected]` if the ready
/// receiver was observed disconnected.  Errors if any receiver in the
/// array is already closed (via `chan/close-recv`).  Never yields and
/// never blocks — this is the building block the Lisp-level
/// `chan/select` uses to retry after a `chan/wait-ready` wake.
fn prim_chan_try_select(args: &[Value]) -> (SignalBits, Value) {
    match with_receivers(&args[0], "chan/try-select", |recvs| {
        let borrows: Vec<_> = recvs.iter().map(|r| r.0.borrow()).collect();
        let mut sel = crossbeam_channel::Select::new();
        let mut rxs: Vec<&crossbeam_channel::Receiver<SendableValue>> =
            Vec::with_capacity(borrows.len());
        for (i, b) in borrows.iter().enumerate() {
            match b.as_ref() {
                Some(rx) => {
                    rxs.push(rx);
                    sel.recv(rx);
                }
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "state-error",
                            format!("chan/try-select: receiver at index {} is closed", i),
                        ),
                    );
                }
            }
        }
        // Bind so the SelectedOperation temporary is dropped before
        // `borrows` at the end of the closure scope.
        let outcome = match sel.try_select() {
            Ok(oper) => {
                let index = oper.index();
                match oper.recv(rxs[index]) {
                    Ok(SendableValue(v)) => {
                        (SIG_OK, Value::array(vec![Value::int(index as i64), v]))
                    }
                    Err(_) => (SIG_OK, Value::array(vec![Value::keyword("disconnected")])),
                }
            }
            Err(_) => (SIG_OK, Value::array(vec![Value::keyword("empty")])),
        };
        outcome
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
}

/// `(chan/wait-ready receivers)` / `(chan/wait-ready receivers timeout-ms)`
///
/// Park the current fiber until any receiver in `receivers` is signaled
/// by a `chan/send` (or sender/receiver close), or until `timeout-ms`
/// elapses.  Three possible returns:
///
/// - `[:ready index msg]` — fast path: after registering the wake fd in
///   every receiver's `WakeList`, a final `try_select` saw a value
///   already in the channel.  No yield happened; the caller can use
///   the returned `index`/`msg` directly without calling
///   `chan/try-select`.
/// - `[:disconnected]` — same fast path, but the ready receiver was
///   disconnected.
/// - `nil` — the primitive yielded; the fiber was parked on the wake
///   fd until POLLIN or timeout fired.  Caller must follow up with
///   `chan/try-select` to actually pick a ready receiver (and re-park
///   with the remaining timeout if the wake turned out to be spurious;
///   the Lisp `chan/select` wrapper handles this).
///
/// Allocates one wake fd (eventfd on Linux, pipe2 elsewhere) and
/// registers it in every receiver's `WakeList`.  A successful
/// `chan/send` on any of those channels writes a wake byte; the
/// scheduler observes POLLIN via `IORING_OP_POLL_ADD` (or `poll(2)` on
/// the thread-pool backend) and resumes this fiber.  The
/// `ChanSelectGuard` carried by the IoRequest deregisters and closes
/// the fd on completion, cancellation, or aborted submission.
fn prim_chan_wait_ready(args: &[Value]) -> (SignalBits, Value) {
    // Parse timeout before any allocation so a bad timeout cleans up
    // nothing.  nil/missing means wait forever.
    let timeout = if args.len() == 2 && !args[1].is_nil() {
        match args[1].as_int() {
            Some(ms) if ms >= 0 => Some(Duration::from_millis(ms as u64)),
            Some(ms) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("chan/wait-ready: timeout must be non-negative, got {}", ms),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "chan/wait-ready: expected integer for timeout, got {}",
                            args[1].type_name()
                        ),
                    ),
                );
            }
        }
    } else {
        None
    };

    match with_receivers(&args[0], "chan/wait-ready", |recvs| {
        let wake_lists: Vec<Arc<WakeList>> = recvs.iter().map(|r| Arc::clone(&r.1)).collect();

        let (poll_fd, wake_fd) = match make_wake_fd() {
            Ok(pair) => pair,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        format!("chan/wait-ready: failed to allocate wake fd: {}", e),
                    ),
                );
            }
        };

        // Register the *wake* fd in every receiver's wake list — the
        // write-side fd (same as poll_fd on Linux's eventfd, distinct
        // on pipe-based platforms).  Doing this *before* the
        // post-register re-check below means any send happening from
        // this moment on writes to our wake fd (counter semantics on
        // eventfd, byte-buffer semantics on pipe), so the upcoming
        // POLL_ADD / poll(2) returns POLLIN immediately even if the
        // kernel hasn't yet armed the poll when the send fires.
        for wl in &wake_lists {
            wl.register(wake_fd);
        }

        // Close the cross-thread race window between the wrapper's first
        // chan/try-select and this register: a send that snuck in
        // between (with an empty wake-list and therefore no signal) is
        // still observed by this re-check.  If we find something
        // ready, do not yield — extract the value and return [:ready i
        // v] so the caller can skip its own chan/try-select call.  A
        // closed receiver here falls through to the yield path; the
        // wake from chan/close-recv will unblock us promptly and the
        // wrapper's chan/try-select reports the closure.
        //
        // Done inside an inner block so the borrows / Select / rxs all
        // drop before we either build the guard early (fast return) or
        // hand it to the yield IoRequest.
        let recheck: Option<Value> = {
            let borrows: Vec<_> = recvs.iter().map(|r| r.0.borrow()).collect();
            let mut sel = crossbeam_channel::Select::new();
            let mut rxs: Vec<&crossbeam_channel::Receiver<SendableValue>> =
                Vec::with_capacity(borrows.len());
            let mut all_open = true;
            for b in borrows.iter() {
                match b.as_ref() {
                    Some(rx) => {
                        rxs.push(rx);
                        sel.recv(rx);
                    }
                    None => {
                        all_open = false;
                        break;
                    }
                }
            }
            if all_open {
                match sel.try_select() {
                    Ok(oper) => {
                        let index = oper.index();
                        Some(match oper.recv(rxs[index]) {
                            Ok(SendableValue(v)) => Value::array(vec![
                                Value::keyword("ready"),
                                Value::int(index as i64),
                                v,
                            ]),
                            Err(_) => Value::array(vec![Value::keyword("disconnected")]),
                        })
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        if let Some(result) = recheck {
            let _guard = ChanSelectGuard {
                poll_fd,
                wake_fd,
                wake_lists,
            };
            return (SIG_OK, result);
        }

        let guard = ChanSelectGuard {
            poll_fd,
            wake_fd,
            wake_lists,
        };
        let cell = ChanSelectGuardCell::new(guard);
        let req = IoRequest::with_timeout(IoOp::ChanSelectPark(cell), Value::NIL, timeout);
        (SIG_YIELD | SIG_IO, req)
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
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
    }
    "chan/send" => prim_chan_send {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Non-blocking send. Returns [:ok], [:full], or [:disconnected].",
        params: &["sender", "msg"],
        category: "chan",
        example: "(chan/send sender 42)",
    }
    "chan/recv" => prim_chan_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking receive. Returns [:ok msg], [:empty], or [:disconnected].",
        params: &["receiver"],
        category: "chan",
        example: "(chan/recv receiver)",
    }
    "chan/clone" => prim_chan_clone {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Clone a sender. Multiple senders can feed the same channel.",
        params: &["sender"],
        category: "chan",
        example: "(chan/clone sender)",
    }
    "chan/close" => prim_chan_close {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a sender. Receivers will get :disconnected after buffered messages drain.",
        params: &["sender"],
        category: "chan",
        example: "(chan/close sender)",
    }
    "chan/close-recv" => prim_chan_close_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a receiver. Senders will get :disconnected on next send.",
        params: &["receiver"],
        category: "chan",
        example: "(chan/close-recv receiver)",
    }
    "chan/try-select" => prim_chan_try_select {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking poll over receivers. Returns [index msg], [:empty], or [:disconnected].",
        params: &["receivers"],
        category: "chan",
        example: "(chan/try-select @[r1 r2])",
    }
    "chan/wait-ready" => prim_chan_wait_ready {
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        arity: Arity::Range(1, 2),
        doc: "Park the current fiber until a receiver is ready, a sender closes, or timeout-ms elapses. Returns nil; caller re-checks with chan/try-select.",
        params: &["receivers", "&opt timeout-ms"],
        category: "chan",
        example: "(chan/wait-ready @[r1 r2] 1000)",
    }
}
