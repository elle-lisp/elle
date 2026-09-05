// audited: 2026-09-05
// src/io/AGENTS.md
//! The per-operation bound a thread-pool worker runs its syscalls under.
//!
//! Two types, one for each side of the handover. [`Bounds`] is what a
//! submission declares: a deadline and a stop pipe. [`OpBound`] is what the
//! worker runs under: it holds the descriptor non-blocking for the operation
//! and turns the declared bounds into waits.

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Descriptors this process holds in non-blocking mode for a worker operation.
///
/// `O_NONBLOCK` belongs to the open file description, so operations that share
/// a descriptor share the flag. The first operation to arrive sets it and
/// records what it found; the last one to leave puts that back. Without the
/// count, a duplex port's read and write operations would clear the flag from
/// under each other and leave the survivor blocking in the kernel.
static NONBLOCKING: LazyLock<Mutex<HashMap<RawFd, NonBlockShare>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// One descriptor's share of the non-blocking flag.
struct NonBlockShare {
    /// Operations currently holding the descriptor non-blocking.
    holders: usize,
    /// True when this process set the flag, so the last holder clears it.
    /// False when the descriptor already carried it and must keep it.
    clear_on_release: bool,
}

/// Why a readiness wait ended.
pub(super) enum Wake {
    /// The descriptor reported the events asked for. Retry the syscall.
    Ready,
    /// The caller's `:timeout` elapsed.
    TimedOut,
    /// `io/cancel` asked this operation to stop.
    Stopped,
}

/// One operation's stop pipe.
///
/// A worker blocked in a syscall cannot be interrupted from another thread,
/// and the alternatives destroy state the port still owns: shutting the
/// descriptor down would break a socket the caller keeps using, and a signal
/// would land wherever the kernel chose. So a cancellable operation waits for
/// readiness and for this pipe together, and cancelling writes one byte.
pub(in crate::io) struct StopPipe {
    /// Polled by the worker alongside its own descriptor.
    pub(in crate::io) read_fd: RawFd,
    /// Written by `CompletionHub::stop`.
    pub(in crate::io) write_fd: RawFd,
}

/// What one submission declares about the wait its operation may enter: how
/// long a wait may take, and the pipe that ends the operation early.
///
/// Every submission passes one, so an operation that can park cannot be written
/// without saying which of three kinds it is — bounded by the caller's
/// `:timeout` and a stop pipe (`CompletionHub::bounds`), [`prompt`], or
/// [`uninterruptible`]. The bounds own the stop pipe's read end and close it
/// with themselves, so a submission that never reaches a worker disposes of the
/// pipe by being dropped.
///
/// [`prompt`]: Bounds::prompt
/// [`uninterruptible`]: Bounds::uninterruptible
pub(in crate::io) struct Bounds {
    /// How long one readiness wait may take. `None` waits indefinitely, which
    /// is what a request that named no timeout asks for.
    timeout: Option<Duration>,
    /// The read end of this operation's stop pipe.
    stop: Option<RawFd>,
}

impl Bounds {
    /// Bound an operation by the caller's `:timeout` and by `stop`, the read
    /// end of its stop pipe. `CompletionHub::bounds` is what pairs the two — it
    /// keeps the write end, which is what lets `io/cancel` reach the worker.
    pub(in crate::io) fn new(timeout: Option<Duration>, stop: Option<RawFd>) -> Bounds {
        Bounds { timeout, stop }
    }

    /// For an operation whose syscalls return without waiting on anything
    /// outside this process: `fsync`, `shutdown`, a datagram `sendto`. There is
    /// no wait to bound, and nothing for a cancel to interrupt.
    pub(in crate::io) fn prompt() -> Bounds {
        Bounds {
            timeout: None,
            stop: None,
        }
    }

    /// For an operation whose syscall cannot be interrupted once entered.
    /// It runs to its own end whatever the caller does, and only its result is
    /// discarded — so its worker thread is held for that whole time. Every use
    /// names the syscall that behaves this way.
    pub(in crate::io) fn uninterruptible() -> Bounds {
        Bounds {
            timeout: None,
            stop: None,
        }
    }

    /// The read end of the stop pipe, for the wait that polls it.
    fn stop(&self) -> Option<RawFd> {
        self.stop
    }
}

impl Drop for Bounds {
    fn drop(&mut self) {
        if let Some(fd) = self.stop.take() {
            // SAFETY: the read end belongs to this operation alone, from the
            // moment `CompletionHub::bounds` opened it.
            unsafe { libc::close(fd) };
        }
    }
}

/// Open a stop pipe, or `None` when the process is out of descriptors — in
/// which case the operation runs uncancellable, exactly as it did before.
pub(in crate::io) fn open_stop_pipe() -> Option<StopPipe> {
    let mut fds = [0 as RawFd; 2];
    // `pipe2` is not on macOS, so set FD_CLOEXEC after the fact.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
        return None;
    }
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Some(StopPipe {
        read_fd: fds[0],
        write_fd: fds[1],
    })
}

/// Bounds one worker operation by the caller's `:timeout` and by
/// cancellation, whatever kind of descriptor it runs on.
///
/// The bound belongs to the operation rather than to the descriptor.
/// `SO_RCVTIMEO`/`SO_SNDTIMEO` bound a socket and a pipe, a fifo and a tty
/// reject them, yet a reader that stops reading fills a pipe exactly as it
/// fills a socket. `poll(2)` accepts every descriptor, so the worker waits
/// there and every descriptor is bounded alike — and the same wait watches the
/// stop pipe, so cancelling ends the operation rather than abandoning it.
///
/// Non-blocking mode is what makes that wait sufficient: a blocking syscall can
/// park again after a poll reports the descriptor ready, while a non-blocking
/// one reports `EAGAIN` and hands the wait back here.
///
/// Each wait carries the caller's whole duration rather than a share of one
/// deadline struck at the start. That is what makes `:timeout` bound each
/// kernel operation instead of the whole call (docs/io.md): a peer that has
/// stalled trips one wait, while a peer that keeps delivering resets the bound
/// every time it does and the transfer finishes however long it takes.
pub(super) struct OpBound {
    fd: RawFd,
    /// What the submission declared, and the owner of the stop pipe's read end
    /// for the operation's lifetime.
    bounds: Bounds,
    /// True while this operation counts as a holder of the non-blocking flag.
    holding: bool,
}

impl OpBound {
    /// Bound an operation that reads from or writes to `fd`. An operation that
    /// can time out or be stopped takes the descriptor non-blocking for its
    /// lifetime, so its syscall reports `EAGAIN` and hands the wait back here
    /// where both endings can be observed. An operation with neither leaves the
    /// descriptor as it found it and blocks in the kernel.
    pub(super) fn new(fd: RawFd, bounds: Bounds) -> Self {
        let holding =
            (bounds.timeout.is_some() || bounds.stop().is_some()) && acquire_nonblocking(fd);
        OpBound {
            fd,
            bounds,
            holding,
        }
    }

    /// Bound an operation that only watches `fd` and never reads or writes it.
    ///
    /// `ev/poll-fd` and the `chan/wait-ready` park report the readiness of a
    /// descriptor somebody else owns — a display connection, a GLib event
    /// source, a channel's wake pipe. Non-blocking mode is what keeps a
    /// *syscall* from parking after a readiness report, and these operations
    /// make no such syscall, so the bound leaves the descriptor exactly as it
    /// found it.
    pub(super) fn watching(fd: RawFd, bounds: Bounds) -> Self {
        OpBound {
            fd,
            bounds,
            holding: false,
        }
    }

    /// Bound an operation with no descriptor of its own: a timer, an `open`, a
    /// child wait. Only the deadline and the stop pipe remain — `poll(2)`
    /// ignores the negative descriptor that stands in for the missing one.
    pub(super) fn detached(bounds: Bounds) -> Self {
        OpBound::watching(-1, bounds)
    }

    /// How long one readiness wait under this bound may take.
    pub(super) fn timeout(&self) -> Option<Duration> {
        self.bounds.timeout
    }

    /// Wait until the descriptor reports `events`, the caller's timeout
    /// elapses, or the operation is stopped.
    ///
    /// A descriptor `poll(2)` rejects reports `Ready`: it cannot report
    /// readiness, so the syscall retries and names the failure itself.
    pub(super) fn wait(&self, events: libc::c_short) -> Wake {
        self.wait_revents(events).0
    }

    /// Wait as [`wait`](Self::wait) does, and report which events fired with
    /// it. `ev/poll-fd` hands that mask straight to its caller, so the mask has
    /// to come from the same `poll(2)` that observed it — asking the descriptor
    /// a second time would report whatever another reader left behind.
    pub(super) fn wait_revents(&self, events: libc::c_short) -> (Wake, libc::c_short) {
        let deadline = self.bounds.timeout.map(|t| Instant::now() + t);
        loop {
            let timeout_ms = match deadline {
                None => -1,
                Some(at) => {
                    let left = at.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return (Wake::TimedOut, 0);
                    }
                    // Round up: a remainder under a millisecond is still time
                    // the caller granted, and rounding it away would report a
                    // timeout the caller did not ask for.
                    let ms = left.as_micros().div_ceil(1000);
                    ms.min(libc::c_int::MAX as u128) as libc::c_int
                }
            };
            let mut pfds = [
                libc::pollfd {
                    fd: self.fd,
                    events,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.bounds.stop().unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let n = if self.bounds.stop().is_some() { 2 } else { 1 };
            let ret = unsafe { libc::poll(pfds.as_mut_ptr(), n, timeout_ms) };
            if ret > 0 {
                // The stop is checked first: a descriptor that is ready and an
                // operation that was cancelled both happened, and the caller
                // asked for the cancellation.
                if pfds[1].revents != 0 {
                    return (Wake::Stopped, 0);
                }
                return (Wake::Ready, pfds[0].revents);
            }
            if ret == 0 {
                return (Wake::TimedOut, 0);
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return (Wake::Ready, 0);
        }
    }

    /// Wait out `slice`, or until the operation is stopped, whichever comes
    /// first. Only the stop pipe is polled, so `Ready` is impossible.
    ///
    /// This is for a syscall the kernel offers no readiness for: an AF_UNIX
    /// `connect` to a listener whose backlog is full reports `EAGAIN` on Linux
    /// and `ECONNREFUSED` on macOS and the BSDs, and gives nothing to wait on
    /// either way, so its retry has to be paced. The stop stays visible
    /// throughout, which a plain sleep would not allow.
    pub(super) fn pause(&self, slice: Duration) -> Wake {
        let mut pfd = libc::pollfd {
            fd: self.bounds.stop().unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        };
        // With no stop pipe there is nothing to watch, and `poll` over zero
        // descriptors is the portable sleep.
        let n = if self.bounds.stop().is_some() { 1 } else { 0 };
        let ms = slice
            .as_micros()
            .div_ceil(1000)
            .min(libc::c_int::MAX as u128) as libc::c_int;
        let ret = unsafe { libc::poll(&mut pfd, n, ms) };
        if ret > 0 && pfd.revents != 0 {
            Wake::Stopped
        } else {
            // A signal cuts the pause short. The caller re-checks its own
            // deadline before pausing again, so a short slice costs nothing.
            Wake::TimedOut
        }
    }

    /// Wait out `duration`, or until the operation is stopped. The timer's
    /// whole wait — there is no descriptor to poll.
    pub(super) fn sleep(&self) -> Wake {
        match self.wait(0) {
            // No descriptor events were asked for, so readiness cannot be why
            // this returned; only the timeout can.
            Wake::Ready => Wake::TimedOut,
            other => other,
        }
    }
}

impl Drop for OpBound {
    fn drop(&mut self) {
        if self.holding {
            release_nonblocking(self.fd);
        }
        // The stop pipe's read end goes with `self.bounds`, dropped after this.
    }
}

/// Take from a descriptor that may have nothing yet: wait for `events` under
/// the operation's bound, attempt the syscall, and repeat while the attempt
/// reports that nothing was there. Returns what `attempt` returned, or
/// `-ECANCELED` / `-ETIMEDOUT` for the two ways an operation ends without it.
///
/// The wait comes first because the syscall must never park. A worker inside a
/// blocking `accept(2)` or `recvfrom(2)` is unreachable — closing the socket
/// does not wake a thread already in the syscall — so the operation would
/// outlive both its deadline and the fiber that asked for it.
///
/// [`OpBound`] takes the descriptor non-blocking for the operation's lifetime,
/// so a readiness another operation consumed first reports `EAGAIN` instead of
/// parking. That and `EINTR` both mean "nothing taken yet": wait again rather
/// than report a failure.
pub(super) fn take_when_ready(
    bound: &OpBound,
    events: libc::c_short,
    mut attempt: impl FnMut() -> isize,
) -> isize {
    loop {
        match bound.wait(events) {
            Wake::Stopped => return -(libc::ECANCELED as isize),
            Wake::TimedOut => return -(libc::ETIMEDOUT as isize),
            Wake::Ready => {}
        }
        let r = attempt();
        if r >= 0 {
            return r;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if !is_would_block(errno) && errno != libc::EINTR {
            return -(errno as isize);
        }
    }
}

/// Wait before asking again, for a syscall the kernel offers no readiness for:
/// an AF_UNIX `connect` to a listener whose backlog is full, a fifo `open`
/// whose other end nobody has opened, a child that has not exited. Waits
/// `pace`, or the rest of `deadline` when that is shorter.
///
/// `Ready` means the pause ended and the caller should ask again; `TimedOut`
/// means the caller's own deadline passed; `Stopped` means `io/cancel` arrived.
/// The stop pipe stays visible throughout, which a plain sleep would not allow.
pub(super) fn pace_retry(bound: &OpBound, deadline: Option<Instant>, pace: Duration) -> Wake {
    let slice = match deadline {
        None => pace,
        Some(at) => {
            let left = at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Wake::TimedOut;
            }
            left.min(pace)
        }
    };
    match bound.pause(slice) {
        Wake::Stopped => Wake::Stopped,
        // `pause` reports the slice's end as `TimedOut`. The caller's own
        // deadline was checked just above, so the slice ending means the
        // operation still has time: ask again.
        _ => Wake::Ready,
    }
}

/// True when `errno` says the descriptor is not ready yet, which is a wait
/// rather than a failure. Every read and write loop treats it that way whether
/// or not it asked for a timeout, so an untimed operation that meets a
/// descriptor another operation made non-blocking waits instead of failing.
pub(super) fn is_would_block(errno: i32) -> bool {
    errno == libc::EAGAIN || errno == libc::EWOULDBLOCK
}

/// Take `fd` non-blocking for one operation and report whether this operation
/// counts as a holder. A descriptor that rejects the flag yields no holder: the
/// syscall then blocks, which is what it did before any bound existed.
fn acquire_nonblocking(fd: RawFd) -> bool {
    let mut held = NONBLOCKING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(share) = held.get_mut(&fd) {
        share.holders += 1;
        return true;
    }
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return false;
    }
    let already = flags & libc::O_NONBLOCK != 0;
    if !already && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return false;
    }
    held.insert(
        fd,
        NonBlockShare {
            holders: 1,
            clear_on_release: !already,
        },
    );
    true
}

/// Release one holder's share, restoring the descriptor's own blocking mode
/// once the last operation on it has finished.
fn release_nonblocking(fd: RawFd) {
    let mut held = NONBLOCKING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(share) = held.get_mut(&fd) else {
        return;
    };
    share.holders -= 1;
    if share.holders > 0 {
        return;
    }
    let clear = share.clear_on_release;
    held.remove(&fd);
    if clear {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
        }
    }
}
