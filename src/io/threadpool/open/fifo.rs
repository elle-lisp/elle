// audited: 2026-11-20
//! The read side of a fifo opened on a pool worker: a blocking open on a helper thread, which a stop or the bound wakes.
//!
//! docs/impl/io/cancel.md
//! docs/io/timeout.md

use super::*;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

/// How long a wake waits for the helper's open to return before it opens the
/// write end again. A helper that had not yet entered its open missed the
/// last writer, and only a later one reaches it.
const WAKE_PACE: Duration = Duration::from_millis(10);

/// Whether `fd` is a fifo.
pub(super) fn is_fifo(fd: RawFd) -> bool {
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, st.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: fstat(2) answered 0, so it filled `st`.
    let mode = unsafe { st.assume_init() }.st_mode;
    mode & libc::S_IFMT == libc::S_IFIFO
}

/// What the helper's open came to, and whether the worker still takes it.
#[derive(Default)]
struct Handover {
    /// The descriptor or `-errno`, once the open returns to a worker waiting
    /// for it.
    result: Option<i32>,
    /// True once the worker answered without the helper. The helper then
    /// closes what its open returns.
    abandoned: bool,
}

/// The state the worker and its helper share. Each thread holds it, so the
/// pipe's ends close with the later of the two, and the helper's write never
/// meets a closed read end.
struct Helper {
    handover: Mutex<Handover>,
    /// The helper writes one byte here once its open returns.
    ready_read: OwnedFd,
    ready_write: OwnedFd,
}

impl Helper {
    /// Record what the helper's open returned, or close it for a worker that
    /// answered already, then say so through the pipe.
    fn returned(&self, fd: i32) {
        {
            let mut handover = self.handover.lock().unwrap_or_else(|e| e.into_inner());
            if handover.abandoned {
                if fd >= 0 {
                    unsafe { libc::close(fd) };
                }
            } else {
                handover.result = Some(fd);
            }
        }
        let byte = 1u8;
        unsafe {
            libc::write(
                self.ready_write.as_raw_fd(),
                &byte as *const u8 as *const libc::c_void,
                1,
            )
        };
    }

    /// Take what the helper's open returned, or leave the helper to close it.
    fn take_or_abandon(&self) -> Option<i32> {
        let mut handover = self.handover.lock().unwrap_or_else(|e| e.into_inner());
        if handover.result.is_none() {
            handover.abandoned = true;
        }
        handover.result.take()
    }
}

/// Wait for a writer to open the fifo at `path`, whose read side `probe` holds
/// open non-blocking, and report a descriptor opened with `flags`.
///
/// A helper thread makes the blocking `open(2)`, which returns when a writer
/// opens the other end, and the worker waits on its pipe under `bounds`. A stop
/// or an expired bound wakes the helper: the worker opens the write end itself
/// and closes it. `probe` is a reader, so that open succeeds rather than
/// answering `ENXIO`. A wake the fifo's permissions refuse leaves the helper
/// waiting for a real writer, and it closes what that open returns.
pub(super) fn open_for_writer(
    bounds: Bounds,
    path: &std::ffi::CStr,
    flags: i32,
    probe: RawFd,
) -> (i32, Vec<u8>) {
    // SAFETY: the open just handed this descriptor to this worker.
    let probe = unsafe { OwnedFd::from_raw_fd(probe) };
    let helper = match ready_pipe() {
        Ok((ready_read, ready_write)) => Arc::new(Helper {
            handover: Mutex::new(Handover::default()),
            ready_read,
            ready_write,
        }),
        Err(errno) => return (-errno, Vec::new()),
    };
    let theirs = Arc::clone(&helper);
    let owned_path = path.to_owned();
    let spawned = std::thread::Builder::new()
        .name("elle-fifo-open".into())
        .spawn(move || theirs.returned(blocking_open(&owned_path, flags)));
    if let Err(e) = spawned {
        return (-e.raw_os_error().unwrap_or(libc::EAGAIN), Vec::new());
    }

    let bound = OpBound::watching(helper.ready_read.as_raw_fd(), bounds);
    let ended = match bound.wait(libc::POLLIN) {
        // The helper writes its byte only after it records the result.
        Wake::Ready => return (helper.take_or_abandon().unwrap_or(-libc::EIO), Vec::new()),
        Wake::Stopped => -libc::ECANCELED,
        Wake::TimedOut => -libc::ETIMEDOUT,
    };
    wake(&helper, path);
    drop(probe);
    (ended, Vec::new())
}

/// Open the write end of the fifo at `path` until the helper's open returns,
/// and close what it returned. When the fifo refuses the write end, leave the
/// helper to close it.
fn wake(helper: &Helper, path: &std::ffi::CStr) {
    loop {
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error();
            if errno == Some(libc::EINTR) {
                continue;
            }
            if let Some(fd) = helper.take_or_abandon() {
                if fd >= 0 {
                    unsafe { libc::close(fd) };
                }
            }
            return;
        }
        unsafe { libc::close(fd) };
        if readable(helper.ready_read.as_raw_fd(), WAKE_PACE) {
            if let Some(fd) = helper.take_or_abandon() {
                if fd >= 0 {
                    unsafe { libc::close(fd) };
                }
            }
            return;
        }
    }
}

/// Open `path` with `flags` and no `O_NONBLOCK`, asking again after `EINTR`:
/// the descriptor, or `-errno`.
fn blocking_open(path: &std::ffi::CStr, flags: i32) -> i32 {
    loop {
        let fd = unsafe { libc::open(path.as_ptr(), flags & !libc::O_NONBLOCK) };
        if fd >= 0 {
            return fd;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        if errno != libc::EINTR {
            return -errno;
        }
    }
}

/// A close-on-exec pipe, or the errno that refused it.
fn ready_pipe() -> Result<(OwnedFd, OwnedFd), i32> {
    let mut fds = [0 as RawFd; 2];
    // `pipe2` is not on macOS, so set FD_CLOEXEC after the fact.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EMFILE));
    }
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    // SAFETY: pipe(2) just handed both descriptors to this thread.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Whether `fd` turns readable within `slice`.
fn readable(fd: RawFd, slice: Duration) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = slice.as_millis().min(libc::c_int::MAX as u128) as libc::c_int;
    unsafe { libc::poll(&mut pfd, 1, ms) > 0 }
}
