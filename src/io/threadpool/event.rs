//! Reads on the descriptors the kernel publishes events through: the inotify
//! or kqueue descriptor behind `fs/watch`, and the signalfd or kqueue
//! descriptor behind `os/sig-watch`.
//!
//! Each of these waits for an event that may never arrive — a directory
//! nothing touches, a signal nobody sends — so each waits under its bound
//! rather than in the read. Every descriptor here is pollable, so the bound
//! needs nothing the other operations do not already have.

use super::*;

/// Read one batch of filesystem watch events from an inotify descriptor.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn watch_read(bound: OpBound, fd: RawFd) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; 4096];
    let ret = take_when_ready(&bound, libc::POLLIN, || unsafe {
        libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
    });
    if ret < 0 {
        return (ret as i32, Vec::new());
    }
    buf.truncate(ret as usize);
    (ret as i32, buf)
}

/// Read one batch of POSIX signal deliveries from a signalfd.
///
/// The signalfd is created with `SFD_NONBLOCK` (see `src/io/sigfd.rs`) so the
/// io_uring path can rely on the kernel's poll-then-read pipeline. Here the
/// bound supplies that pipeline: it waits for `POLLIN` and the read reports
/// `EAGAIN` if another reader drained the queue first.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn sigfd_read(
    bound: OpBound,
    trace: &crate::config::TraceCell,
    fd: RawFd,
) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    posix_trace(trace, format_args!("linux: sigfd_read entered fd={}", fd));
    let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
    let mut buf = vec![0u8; entry_size * 8];
    let ret = take_when_ready(&bound, libc::POLLIN, || unsafe {
        libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
    });
    posix_trace(trace, format_args!("linux: sigfd_read returning n={}", ret));
    if ret < 0 {
        return (ret as i32, Vec::new());
    }
    buf.truncate(ret as usize);
    (ret as i32, buf)
}

/// Read one batch of filesystem watch events from a kqueue descriptor.
/// Encodes results as `(fd:i32, fflags:u32)` little-endian pairs for
/// `FsWatcher::parse_events()`.
#[cfg(target_os = "macos")]
pub(super) fn watch_read(bound: OpBound, kq: RawFd) -> (i32, Vec<u8>) {
    let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
    let n = take_kevents(&bound, kq, &mut eventlist);
    if n < 0 {
        return (n as i32, Vec::new());
    }
    let mut data = Vec::with_capacity(n as usize * 8);
    for event in &eventlist[..n as usize] {
        data.extend_from_slice(&(event.ident as i32).to_le_bytes());
        data.extend_from_slice(&event.fflags.to_le_bytes());
    }
    (data.len() as i32, data)
}

/// Read one batch of POSIX signal deliveries from a kqueue descriptor
/// registered with `EVFILT_SIGNAL`. Encodes results as `(signum:i32,
/// count:u32)` little-endian pairs for `SignalReceiver::parse_events()`.
///
/// `signals` is the set the receiver registered with kqueue. The worker
/// `pthread_sigmask`-UNBLOCKs them on itself before waiting so the kernel can
/// pick this thread as the delivery target — kqueue's `EVFILT_SIGNAL` is driven
/// by the in-kernel delivery path, not by signal generation. With every other
/// thread in the process blocking the signal (the worker default plus the main
/// thread's `os/sig-watch` mask), the kernel parks the signal on the process
/// pending list, no thread is selected for delivery, and the knote never
/// activates.
///
/// `SignalReceiver::new` installs a process-wide no-op sigaction handler for
/// each watched signal at refcount 0 → 1 (and restores at 1 → 0), so the
/// delivery the kernel makes to this worker is a harmless return through the
/// trampoline rather than the default disposition (Term for SIGUSR1, and so on).
#[cfg(target_os = "macos")]
pub(super) fn kq_sig_read(
    bound: OpBound,
    trace: &crate::config::TraceCell,
    kq: RawFd,
    signals: &[libc::c_int],
) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    posix_trace(
        trace,
        format_args!("macos: kq_sig_read entered kq={} signals={:?}", kq, signals),
    );
    // Unblock the watched signals on this thread for this read, and no longer:
    // a worker runs the operations that come after this one too, and every
    // other operation needs the thread unselectable for delivery.
    let unblocked = Unblocked::on_this_thread(signals);
    posix_trace(
        trace,
        format_args!("macos: kq_sig_read SIG_UNBLOCK ret={}", unblocked.ret),
    );

    let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
    let n = take_kevents(&bound, kq, &mut eventlist);
    posix_trace(trace, format_args!("macos: kq_sig_read took n={}", n));
    if n < 0 {
        return (n as i32, Vec::new());
    }
    let mut data = Vec::with_capacity(n as usize * 8);
    for event in &eventlist[..n as usize] {
        let signum = event.ident as i32;
        let count = event.data as u32;
        posix_trace(
            trace,
            format_args!("macos: kq_sig_read event signum={} count={}", signum, count),
        );
        data.extend_from_slice(&signum.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
    }
    (data.len() as i32, data)
}

/// A set of signals this thread has unblocked, blocked again when dropped.
///
/// Restoring is a plain `SIG_BLOCK` of the same set because a worker starts
/// with every asynchronous signal blocked (`mask_all_signals_on_this_thread`),
/// so blocking what this unblocked is exactly the mask it found.
#[cfg(target_os = "macos")]
pub(super) struct Unblocked {
    set: libc::sigset_t,
    /// What `pthread_sigmask` reported, for the operation's trace.
    ret: libc::c_int,
}

#[cfg(target_os = "macos")]
impl Unblocked {
    pub(super) fn on_this_thread(signals: &[libc::c_int]) -> Self {
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut set) };
        for &s in signals {
            unsafe { libc::sigaddset(&mut set, s) };
        }
        let ret = unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut()) };
        Unblocked { set, ret }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Unblocked {
    fn drop(&mut self) {
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &self.set, std::ptr::null_mut()) };
    }
}

/// Take one batch of events from `kq` under the operation's bound.
///
/// A kqueue descriptor is pollable, so the bound does the waiting and the
/// `kevent` call carries a zero timeout. A `kevent` with a null timeout parks
/// this worker where neither the caller's deadline nor `io/cancel` can reach
/// it, exactly as a blocking `accept(2)` does.
///
/// A zero-event return means another reader took the readiness first — wait
/// again rather than report an empty batch, which the completion would parse as
/// a delivery of nothing.
#[cfg(target_os = "macos")]
fn take_kevents(bound: &OpBound, kq: RawFd, eventlist: &mut [libc::kevent]) -> isize {
    const NOW: libc::timespec = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    loop {
        match bound.wait(libc::POLLIN) {
            Wake::Stopped => return -(libc::ECANCELED as isize),
            Wake::TimedOut => return -(libc::ETIMEDOUT as isize),
            Wake::Ready => {}
        }
        let n = unsafe {
            libc::kevent(
                kq,
                std::ptr::null(),
                0,
                eventlist.as_mut_ptr(),
                eventlist.len() as i32,
                &NOW,
            )
        };
        if n > 0 {
            return n as isize;
        }
        if n == 0 {
            continue;
        }
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
        // The no-op signal handler ran without SA_RESTART, or some other signal
        // interrupted the call. The knote state survives EINTR, so a later
        // kevent picks up the same event.
        if errno == libc::EINTR {
            continue;
        }
        return -(errno as isize);
    }
}
