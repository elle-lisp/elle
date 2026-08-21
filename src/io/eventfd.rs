//! Bridge eventfd helpers (Linux).
//!
//! An eventfd is the wake primitive that lets an off-ring worker raise an edge
//! the io_uring wait observes. The scheduler arms a standing
//! `POLL_ADD(eventfd, POLLIN)` on the ring; whenever the counter is non-zero
//! the poll completes with a CQE, so a thread-pool / stdin worker that posts no
//! ring CQE of its own can still wake the single `io_uring_enter`.
//!
//! `create` opens the fd; `signal` (worker side) bumps the counter after it has
//! published to the hub channel; `drain` (scheduler side) reads the counter
//! back to 0 so the next armed `POLL_ADD` blocks instead of completing on a
//! stale count. The non-semaphore eventfd coalesces — N signals raise the
//! counter to N and a single `drain` resets it — so a burst of completions
//! costs at most one spurious wake.
//!
//! This is the same primitive `chan::make_wake_fd`/`wake_fd_signal` use to wake
//! the scheduler from a `chan/send`; both route their Linux eventfd syscalls
//! through here so there is one definition of each operation.

use std::os::unix::io::RawFd;

/// Open a non-blocking, close-on-exec eventfd with an initial count of 0.
pub(crate) fn create() -> std::io::Result<RawFd> {
    // SAFETY: eventfd(2) with valid flags returns a new fd or -1/errno.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(fd)
}

/// Raise the edge by adding 1 to the counter. Returns the `write(2)` result so
/// a caller can trace it. Failures are benign for the wake protocol — EAGAIN
/// only on counter overflow at `u64::MAX - 1` (a parked poll has long since
/// observed POLLIN), EBADF only after teardown closed the fd (no one is
/// waiting). The 8-byte write to an eventfd never partially completes.
pub(crate) fn signal(fd: RawFd) -> isize {
    let one: u64 = 1;
    // SAFETY: an 8-byte write of a u64 is the eventfd ABI.
    unsafe {
        libc::write(
            fd,
            &one as *const u64 as *const libc::c_void,
            std::mem::size_of::<u64>(),
        )
    }
}

/// Reset the counter to 0 by reading its accumulated value, which the kernel
/// returns and zeroes atomically. `EFD_NONBLOCK` keeps this from blocking if a
/// concurrent reader already drained it to 0 (EAGAIN). The value is discarded —
/// the bridge cares only that an edge arrived, not how many coalesced.
pub(crate) fn drain(fd: RawFd) {
    let mut buf: u64 = 0;
    // SAFETY: an 8-byte read into a u64 is the eventfd ABI; result ignored.
    unsafe {
        libc::read(
            fd,
            &mut buf as *mut u64 as *mut libc::c_void,
            std::mem::size_of::<u64>(),
        );
    }
}
