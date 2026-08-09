//! The per-operation bound a thread-pool worker runs its syscalls under.

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
    /// How long one readiness wait may take. `None` waits indefinitely, which
    /// is what a request that named no timeout asks for.
    timeout: Option<Duration>,
    /// True while this operation counts as a holder of the non-blocking flag.
    holding: bool,
    /// The read end of this operation's stop pipe, owned for the operation's
    /// lifetime and closed with it.
    stop_fd: Option<RawFd>,
}

impl OpBound {
    /// Bound an operation on `fd`. An operation that can time out or be
    /// stopped takes the descriptor non-blocking for its lifetime, so its
    /// syscall reports `EAGAIN` and hands the wait back here where both
    /// endings can be observed. An operation with neither leaves the
    /// descriptor as it found it and blocks in the kernel.
    pub(super) fn new(fd: RawFd, timeout: Option<Duration>, stop_fd: Option<RawFd>) -> Self {
        let holding = (timeout.is_some() || stop_fd.is_some()) && acquire_nonblocking(fd);
        OpBound {
            fd,
            timeout,
            holding,
            stop_fd,
        }
    }

    /// Wait until the descriptor reports `events`, the caller's timeout
    /// elapses, or the operation is stopped.
    ///
    /// A descriptor `poll(2)` rejects reports `Ready`: it cannot report
    /// readiness, so the syscall retries and names the failure itself.
    pub(super) fn wait(&self, events: libc::c_short) -> Wake {
        let deadline = self.timeout.map(|t| Instant::now() + t);
        loop {
            let timeout_ms = match deadline {
                None => -1,
                Some(at) => {
                    let left = at.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Wake::TimedOut;
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
                    fd: self.stop_fd.unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let n = if self.stop_fd.is_some() { 2 } else { 1 };
            let ret = unsafe { libc::poll(pfds.as_mut_ptr(), n, timeout_ms) };
            if ret > 0 {
                // The stop is checked first: a descriptor that is ready and an
                // operation that was cancelled both happened, and the caller
                // asked for the cancellation.
                if pfds[1].revents != 0 {
                    return Wake::Stopped;
                }
                return Wake::Ready;
            }
            if ret == 0 {
                return Wake::TimedOut;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return Wake::Ready;
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
        if let Some(fd) = self.stop_fd {
            // SAFETY: the worker owns the read end for the operation's lifetime.
            unsafe { libc::close(fd) };
        }
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
