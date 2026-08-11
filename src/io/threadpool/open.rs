//! Opening a file.

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
    let deadline = bound.timeout().map(|t| std::time::Instant::now() + t);
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
