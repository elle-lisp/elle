// audited: 2026-11-20
//! Opening a file on a pool worker, in a way the caller's bound and a cancel can both end.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/cancel.md

use super::*;

mod fifo;

/// How long an open that reported `ENXIO` waits before trying again. A fifo
/// opened for writing reports that until a reader opens the other end, and
/// offers no readiness to wait on, so the retry is paced.
const OPEN_RETRY_PACE: Duration = Duration::from_millis(10);

/// Open `path` and report the descriptor, or `-errno`.
///
/// `O_NONBLOCK` is what makes the open answerable. A plain `open(2)` on a fifo
/// parks this worker until the other end opens, which it need never do, and
/// neither the caller's `:timeout` nor `io/cancel` can reach a thread inside
/// the syscall. With the flag the kernel reports instead of parking: the write
/// side reports `ENXIO` until a reader arrives, which is a wait this bound can
/// hold. The read side opens at once, and a read-only open that finds a fifo
/// then waits for a writer in `fifo`.
///
/// The descriptor is handed back in the mode the caller asked for — the flag is
/// cleared again — so everything downstream sees the file it opened. `OpBound`
/// is what sets non-blocking mode per operation from then on.
///
/// A regular file, a directory and a symlink are ready by definition, so the
/// flag changes nothing for them. It changes the open of a device that waits on
/// its line, such as a serial port with no carrier. That open reports at once
/// rather than parking a worker for as long as the line stays down.
pub(super) fn open(bounds: Bounds, path: &std::ffi::CStr, flags: i32, mode: u32) -> (i32, Vec<u8>) {
    if flags & libc::O_ACCMODE == libc::O_RDONLY {
        return match try_open(path, flags, mode) {
            Ok(fd) if fifo::is_fifo(fd) => fifo::open_for_writer(bounds, path, flags, fd),
            Ok(fd) => handed_back(fd, flags),
            Err(errno) => (-errno, Vec::new()),
        };
    }
    let bound = OpBound::detached(bounds);
    let deadline = bound.end();
    loop {
        let errno = match try_open(path, flags, mode) {
            Ok(fd) => return handed_back(fd, flags),
            Err(errno) => errno,
        };
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

/// Open `path` non-blocking, asking again after `EINTR`: the descriptor, or the
/// errno the open failed with.
fn try_open(path: &std::ffi::CStr, flags: i32, mode: u32) -> Result<RawFd, i32> {
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
            return Ok(fd);
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno != libc::EINTR {
            return Err(errno);
        }
    }
}

/// Report `fd`, cleared of the `O_NONBLOCK` the open added unless `flags`
/// asked for it.
fn handed_back(fd: RawFd, flags: i32) -> (i32, Vec<u8>) {
    if flags & libc::O_NONBLOCK == 0 {
        let got = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if got >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, got & !libc::O_NONBLOCK) };
        }
    }
    (fd, Vec::new())
}
