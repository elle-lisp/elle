// audited: 2026-10-22
//! The operations a pool worker runs on a file named by its path, and what each reports.
//!
//! src/io/AGENTS.md

use crate::io::request::FileOp;

/// Run `op` to its result: 0 and the bytes it read, or `-errno` and the text
/// of the error, which the completion reports.
///
/// Every operation but a sync goes through blocking `std::fs` calls, which
/// nothing interrupts; the submission says so with `Bounds::uninterruptible`.
pub(super) fn run(op: FileOp) -> (i32, Vec<u8>) {
    use std::io::Write;

    let done = |r: std::io::Result<()>| match r {
        Ok(()) => (0, Vec::new()),
        Err(e) => failed(&e),
    };
    match op {
        FileOp::Read { path } | FileOp::Lines { path } => match std::fs::read(&path) {
            Ok(bytes) => (0, bytes),
            Err(e) => failed(&e),
        },
        FileOp::Write { path, data } => done(std::fs::write(&path, &data)),
        FileOp::Append { path, data } => done(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .and_then(|mut f| f.write_all(&data)),
        ),
        FileOp::Copy { from, to } => done(std::fs::copy(&from, &to).map(|_| ())),
        FileOp::Sync { path } => match std::ffi::CString::new(path) {
            Ok(path) => sync(&path),
            Err(_) => (-libc::EINVAL, Vec::new()),
        },
    }
}

/// What a failure reports: `-errno`, or `-EINVAL` for an error the OS did not
/// report, such as a copy whose source is not a regular file, and the error's
/// own text.
fn failed(e: &std::io::Error) -> (i32, Vec<u8>) {
    let errno = e.raw_os_error().unwrap_or(libc::EINVAL);
    (-errno, e.to_string().into_bytes())
}

/// The errno of the last failed system call on this thread.
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(1)
}

/// Whether a sync accepts a descriptor whose `st_mode` is `mode`: a regular
/// file or a directory, and nothing else.
pub(super) fn syncable(mode: libc::mode_t) -> bool {
    matches!(mode & libc::S_IFMT, libc::S_IFREG | libc::S_IFDIR)
}

/// Flush `path` to stable storage, and report 0 or `-errno`.
///
/// The open is read-only, which `fsync(2)` accepts on a file and on a
/// directory alike, and non-blocking for the reason `open::open` is: a plain
/// open of a fifo for reading parks this worker until a writer arrives, and
/// nothing reaches a thread inside the syscall. Anything but a file or a
/// directory then answers `-EINVAL` before `fsync(2)`, which refuses a fifo on
/// Linux and syncs one on macOS.
fn sync(path: &std::ffi::CStr) -> (i32, Vec<u8>) {
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
        let errno = last_errno();
        if errno != libc::EINTR {
            return (-errno, Vec::new());
        }
    };
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) } != 0 {
        return (-last_errno(), Vec::new());
    }
    // SAFETY: fstat(2) answered 0, so it filled `st`.
    if !syncable(unsafe { st.assume_init() }.st_mode) {
        return (-libc::EINVAL, Vec::new());
    }
    loop {
        if unsafe { libc::fsync(fd.as_raw_fd()) } == 0 {
            return (0, Vec::new());
        }
        let errno = last_errno();
        if errno != libc::EINTR {
            return (-errno, Vec::new());
        }
    }
}
