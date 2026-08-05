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

/// Bounds one worker operation by the caller's `:timeout`, whatever kind of
/// descriptor it runs on.
///
/// The bound belongs to the operation rather than to the descriptor.
/// `SO_RCVTIMEO`/`SO_SNDTIMEO` bound a socket and a pipe, a fifo and a tty
/// reject them, yet a reader that stops reading fills a pipe exactly as it
/// fills a socket. `poll(2)` accepts every descriptor, so the worker waits
/// there and every descriptor is bounded alike.
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
}

impl OpBound {
    /// Bound an operation on `fd`. A timed operation takes the descriptor
    /// non-blocking for its lifetime; an untimed one leaves the descriptor as
    /// it found it and blocks in the kernel, which is the wait it asked for.
    pub(super) fn new(fd: RawFd, timeout: Option<Duration>) -> Self {
        let holding = timeout.is_some() && acquire_nonblocking(fd);
        OpBound {
            fd,
            timeout,
            holding,
        }
    }

    /// Wait until the descriptor reports `events`. False is the caller's
    /// timeout expiring, and the only reason this reports false.
    ///
    /// A descriptor `poll(2)` rejects reports true: it cannot report readiness,
    /// so the syscall retries and names the failure itself.
    pub(super) fn wait(&self, events: libc::c_short) -> bool {
        let deadline = self.timeout.map(|t| Instant::now() + t);
        loop {
            let timeout_ms = match deadline {
                None => -1,
                Some(at) => {
                    let left = at.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return false;
                    }
                    // Round up: a remainder under a millisecond is still time
                    // the caller granted, and rounding it away would report a
                    // timeout the caller did not ask for.
                    let ms = left.as_micros().div_ceil(1000);
                    ms.min(libc::c_int::MAX as u128) as libc::c_int
                }
            };
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if ret > 0 {
                return true;
            }
            if ret == 0 {
                return false;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return true;
        }
    }
}

impl Drop for OpBound {
    fn drop(&mut self) {
        if self.holding {
            release_nonblocking(self.fd);
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
