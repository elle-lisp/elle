// audited: 2026-10-14
//! Opening or syncing a file on a pool worker, without parking the worker where no bound or cancel reaches it.
//!
//! src/io/AGENTS.md

use super::*;

/// How long an open that reported `ENXIO` waits before trying again. A fifo
/// opened for writing reports that until a reader opens the other end, and
/// offers no readiness to wait on, so the retry is paced.
const OPEN_RETRY_PACE: Duration = Duration::from_millis(10);

/// Open `path` and report the descriptor, or `-errno`.
///
/// `O_NONBLOCK` is what makes the open answerable. A plain `open(2)` on a fifo
/// parks this worker until the other end opens, which it need never do, and
/// neither the caller's `:timeout` nor `io/cancel` can reach a thread inside
/// the syscall. With the flag the kernel reports instead of parking: the read
/// side opens at once, and the write side reports `ENXIO` until a reader
/// arrives, which is a wait this bound can hold.
///
/// The descriptor is handed back in the mode the caller asked for — the flag is
/// cleared again — so everything downstream sees the file it opened. `OpBound`
/// is what sets non-blocking mode per operation from then on.
///
/// A regular file, a directory and a symlink are ready by definition, so the
/// flag changes nothing for them. It changes the open of a device that waits on
/// its line — a serial port with no carrier — which then reports at once rather
/// than parking a worker for as long as the line stays down.
pub(super) fn open(bound: OpBound, path: &std::ffi::CStr, flags: i32, mode: u32) -> (i32, Vec<u8>) {
    let deadline = bound.end();
    loop {
        let fd = unsafe {
            libc::openat(
                libc::AT_FDCWD,
                path.as_ptr(),
                flags | libc::O_NONBLOCK,
                mode as libc::c_uint,
            )
        };
        if fd >= 0 {
            if flags & libc::O_NONBLOCK == 0 {
                let got = unsafe { libc::fcntl(fd, libc::F_GETFL) };
                if got >= 0 {
                    unsafe { libc::fcntl(fd, libc::F_SETFL, got & !libc::O_NONBLOCK) };
                }
            }
            return (fd, Vec::new());
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno == libc::EINTR {
            continue;
        }
        // A fifo opened for writing, with nobody reading it yet. Every other
        // failure is the open's own answer.
        if errno != libc::ENXIO {
            return (-errno, Vec::new());
        }
        match pace_retry(&bound, deadline, OPEN_RETRY_PACE) {
            Wake::Ready => {}
            Wake::Stopped => return (-libc::ECANCELED, Vec::new()),
            Wake::TimedOut => return (-libc::ETIMEDOUT, Vec::new()),
        }
    }
}

/// Flush `path` to stable storage, and report 0 or `-errno`.
///
/// The open is read-only, which `fsync(2)` accepts on a file and on a
/// directory alike, and non-blocking for the reason `open` is: a plain open of
/// a fifo for reading parks this worker until a writer arrives, and nothing
/// reaches a thread inside the syscall. A fifo then fails in `fsync(2)`, at
/// once.
pub(super) fn sync(path: &std::ffi::CStr) -> (i32, Vec<u8>) {
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

    let fd = loop {
        let fd = unsafe {
            libc::openat(
                libc::AT_FDCWD,
                path.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd >= 0 {
            // SAFETY: the kernel just handed this descriptor to this worker.
            break unsafe { OwnedFd::from_raw_fd(fd) };
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno != libc::EINTR {
            return (-errno, Vec::new());
        }
    };
    loop {
        if unsafe { libc::fsync(fd.as_raw_fd()) } == 0 {
            return (0, Vec::new());
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno != libc::EINTR {
            return (-errno, Vec::new());
        }
    }
}
