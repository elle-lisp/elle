// audited: 2026-09-30
//! The wake a parked `chan/select` waits on: each channel's list of wake fds, the fd pair, and the guard that frees them.
//!
//! docs/threads.md
//!
//! `chan/select` cannot use crossbeam's blocking `Select::select_timeout`
//! because that parks the OS thread on which the fiber scheduler runs —
//! starving any `ev/spawn`'d producer fiber that would have unblocked the
//! select.  Instead each channel carries a shared `WakeList` of wake fds.  A
//! selecting fiber allocates a wake fd, registers it in every candidate
//! receiver's `WakeList`, and yields with `IoOp::ChanSelectPark` — the
//! scheduler waits on the fd via `IORING_OP_POLL_ADD` (or `poll(2)` on the
//! thread-pool backend), exactly like `ev/poll-fd`.  `chan/send`, after a
//! successful `try_send`, signals every registered fd so any parked selector
//! wakes and re-tries.  Cross-thread `chan/send` (from `sys/spawn`) wakes the
//! scheduler thread the same way — the write is thread-safe and the kernel
//! poll notices it.

use std::cell::RefCell;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    // benign, because trace lines are diagnostic only.
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
    pub(super) fn register(&self, wake_fd: RawFd) {
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
pub(super) fn make_wake_fd(trace: &crate::config::TraceCell) -> std::io::Result<(RawFd, RawFd)> {
    // One non-blocking, close-on-exec eventfd; poll and wake share the one fd on
    // Linux. The same `crate::io::eventfd::create` backs the io backend's bridge.
    let fd = crate::io::eventfd::create()?;
    chan_trace(trace, format_args!("alloc eventfd={}", fd));
    Ok((fd, fd))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn make_wake_fd(trace: &crate::config::TraceCell) -> std::io::Result<(RawFd, RawFd)> {
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
    /// The guard over the pair `make_wake_fd` answered, whose `wake_fd` is
    /// registered in each of `wake_lists`.
    pub(super) fn new(
        poll_fd: RawFd,
        wake_fd: RawFd,
        wake_lists: Vec<Arc<WakeList>>,
        trace: crate::config::TraceCell,
    ) -> ChanSelectGuard {
        ChanSelectGuard {
            poll_fd,
            wake_fd,
            wake_lists,
            trace,
        }
    }

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
