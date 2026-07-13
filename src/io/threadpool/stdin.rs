use super::*;

/// Main worker loop. Multiplexes:
///   - `request_rx`: incoming `(id, op_kind)` requests.
///   - `shutdown_read_fd`: byte arrival means caller asked us to die.
///   - fd 0: actual stdin input for the in-flight request.
///
/// We don't have a portable way to `select` simultaneously on a
/// crossbeam channel and a raw fd, so the loop alternates: idle ticks
/// poll the shutdown fd alongside a short `recv_timeout` on the
/// channel; the active state (mid-read) uses `poll(2)` on fd 0 plus
/// the shutdown fd.
pub(super) fn stdin_thread_loop(
    request_rx: crossbeam_channel::Receiver<StdinRequest>,
    completion_tx: crossbeam_channel::Sender<RawCompletion>,
    eventfd: Option<RawFd>,
    shutdown_read_fd: RawFd,
) {
    use std::time::Duration;
    let mut leftover: Vec<u8> = Vec::new();
    loop {
        if shutdown_signalled(shutdown_read_fd) {
            return;
        }
        let req = match request_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => r,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        };
        let result = match req.op_kind {
            StdinOpKind::ReadLine => read_line_with_cancel(shutdown_read_fd, &mut leftover),
            StdinOpKind::Read { count } => {
                read_n_with_cancel(shutdown_read_fd, count, &mut leftover)
            }
            StdinOpKind::ReadAll => read_all_with_cancel(shutdown_read_fd, &mut leftover),
        };
        let was_cancelled = matches!(&result, Err(s) if s == STDIN_CLOSED_MSG);
        publish_completion(
            &completion_tx,
            eventfd,
            RawCompletion::Stdin(StdinCompletion { id: req.id, result }),
        );
        if was_cancelled {
            // Drain any further queued requests as cancelled so their
            // submitters see a completion rather than hanging.
            while let Ok(r) = request_rx.try_recv() {
                publish_completion(
                    &completion_tx,
                    eventfd,
                    RawCompletion::Stdin(StdinCompletion {
                        id: r.id,
                        result: Err(STDIN_CLOSED_MSG.to_string()),
                    }),
                );
            }
            return;
        }
    }
}

/// Non-blocking peek at the shutdown pipe. Returns true if any byte
/// is present (we don't bother to drain — `read_*_with_cancel` will
/// see the revents in its next poll).
pub(super) fn shutdown_signalled(shutdown_read_fd: RawFd) -> bool {
    let mut pfd = libc::pollfd {
        fd: shutdown_read_fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
    ret > 0 && pfd.revents != 0
}

/// Poll fd 0 and `shutdown_read_fd` simultaneously. Returns `Ok(true)`
/// when stdin has input available, `Ok(false)` when shutdown was
/// signalled, `Err` on real syscall errors. The shutdown branch wins
/// any race so close-then-arrive-input doesn't leak data into a
/// completion the caller will never read.
pub(super) fn poll_stdin_or_shutdown(shutdown_read_fd: RawFd) -> Result<bool, String> {
    loop {
        let mut pfds = [
            libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: shutdown_read_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(format!("poll: {}", err));
        }
        // Shutdown wins races against pending stdin input.
        if pfds[1].revents != 0 {
            return Ok(false);
        }
        if pfds[0].revents != 0 {
            return Ok(true);
        }
        // No revents but poll > 0 should not happen; loop defensively.
    }
}

/// One iteration of "wait + read" against fd 0 with shutdown
/// observation. Returns:
///   - `Ok(Some(bytes))` — fresh bytes read.
///   - `Ok(None)` — EOF.
///   - `Err(STDIN_CLOSED_MSG)` — shutdown was signalled.
///   - `Err(other)` — real I/O error.
pub(super) fn read_chunk_with_cancel(
    shutdown_read_fd: RawFd,
    max: usize,
) -> Result<Option<Vec<u8>>, String> {
    if !poll_stdin_or_shutdown(shutdown_read_fd)? {
        return Err(STDIN_CLOSED_MSG.to_string());
    }
    let mut buf = vec![0u8; max];
    loop {
        let ret = unsafe { libc::read(0, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EINTR) | Some(libc::EAGAIN) => {
                    // Re-poll: input may have been consumed by another
                    // reader, or the signal was harmless.
                    if !poll_stdin_or_shutdown(shutdown_read_fd)? {
                        return Err(STDIN_CLOSED_MSG.to_string());
                    }
                    continue;
                }
                _ => return Err(format!("read: {}", err)),
            }
        }
        if ret == 0 {
            return Ok(None);
        }
        buf.truncate(ret as usize);
        return Ok(Some(buf));
    }
}

pub(super) fn read_line_with_cancel(
    shutdown_read_fd: RawFd,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    // If a previous read consumed past a newline, the bytes after the
    // newline went into `leftover`. Serve from there first.
    if let Some(nl) = leftover.iter().position(|&b| b == b'\n') {
        let mut line: Vec<u8> = leftover.drain(..=nl).collect();
        line.pop(); // drop \n
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        return Ok(line);
    }
    // Read chunks until newline or EOF.
    let mut accum = std::mem::take(leftover);
    loop {
        match read_chunk_with_cancel(shutdown_read_fd, 4096)? {
            Some(bytes) => {
                accum.extend_from_slice(&bytes);
                if let Some(nl) = accum.iter().position(|&b| b == b'\n') {
                    let mut line: Vec<u8> = accum.drain(..=nl).collect();
                    line.pop(); // drop \n
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    *leftover = accum;
                    return Ok(line);
                }
            }
            None => {
                // EOF: return whatever we have. `Ok(Vec::new())`
                // signals EOF to the completion handler.
                return Ok(accum);
            }
        }
    }
}

pub(super) fn read_n_with_cancel(
    shutdown_read_fd: RawFd,
    count: usize,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    // Drain leftover bytes first. `port/read` returns "up to N bytes"
    // per POSIX semantics, so if leftover has anything we return that
    // immediately (matching how std::io::stdin().lock().read worked:
    // a single read syscall).
    if !leftover.is_empty() {
        let take = leftover.len().min(count);
        let chunk: Vec<u8> = leftover.drain(..take).collect();
        return Ok(chunk);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    match read_chunk_with_cancel(shutdown_read_fd, count)? {
        Some(bytes) => Ok(bytes),
        None => Ok(Vec::new()), // EOF
    }
}

pub(super) fn read_all_with_cancel(
    shutdown_read_fd: RawFd,
    leftover: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    let mut accum = std::mem::take(leftover);
    loop {
        match read_chunk_with_cancel(shutdown_read_fd, 4096)? {
            Some(bytes) => accum.extend_from_slice(&bytes),
            None => return Ok(accum),
        }
    }
}

/// Blocking read on an inotify fd (Linux/Android).
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn watch_read_blocking(fd: RawFd) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; 4096];
    let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if ret < 0 {
        (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        )
    } else {
        buf.truncate(ret as usize);
        (ret as i32, buf)
    }
}

/// Blocking read on a signalfd (Linux). Reads up to 8 signalfd_siginfo
/// structs per call; signalfd has no shutdown(2)-equivalent, so a cancel
/// triggered by the scheduler closes the signalfd, which makes this
/// read return 0 (EOF) and the receiver is dead.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn sigfd_read_blocking(trace: &crate::config::TraceCell, fd: RawFd) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    // The signalfd is created with SFD_NONBLOCK (see src/io/sigfd.rs) so the
    // io_uring path can rely on the kernel's poll-then-read pipeline. The
    // threadpool path has no such pipeline: a bare read(2) on a non-blocking
    // fd before the signal arrives returns -1/EAGAIN. Wait for POLLIN first,
    // looping on EINTR. POLLHUP on signalfd is unusual (no shutdown(2)
    // analogue) but we treat it as EOF for parity with WatchRead.
    posix_trace(
        trace,
        format_args!("linux: sigfd_read_blocking entered fd={}", fd),
    );
    let entry_size = std::mem::size_of::<libc::signalfd_siginfo>();
    loop {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        posix_trace(
            trace,
            format_args!("linux: sigfd_read_blocking poll(fd={}, POLLIN, -1)", fd),
        );
        let pret = unsafe { libc::poll(&mut pfd, 1, -1) };
        if pret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(
                trace,
                format_args!("linux: sigfd_read_blocking poll errno={}", errno),
            );
            if errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        posix_trace(
            trace,
            format_args!(
                "linux: sigfd_read_blocking poll returned, revents=0x{:x}",
                pfd.revents
            ),
        );
        let mut buf = vec![0u8; entry_size * 8];
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(
                trace,
                format_args!("linux: sigfd_read_blocking read errno={}", errno),
            );
            // Racy: a different reader may have drained the queue between
            // poll and read. Loop back to poll.
            if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK || errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        buf.truncate(ret as usize);
        posix_trace(
            trace,
            format_args!("linux: sigfd_read_blocking returning n={}", ret),
        );
        return (ret as i32, buf);
    }
}

/// Blocking kevent() on a kqueue fd registered with EVFILT_SIGNAL (macOS).
/// Encodes results as (signum:i32, count:u32) LE pairs for
/// SignalReceiver::parse_events().
///
/// `signals` is the set the receiver registered with kqueue. The worker
/// `pthread_sigmask`-UNBLOCKs them on itself before kevent() so the
/// kernel can pick this thread as the delivery target — kqueue's
/// `EVFILT_SIGNAL` is driven by the in-kernel delivery path, not by
/// signal generation. With every other thread in the process blocking
/// the signal (the threadpool worker default + the main thread's
/// `os/sig-watch` mask), parking SIGUSR1 on the process pending list
/// without any unblocked thread leaves no delivery path and the knote
/// never activates — exactly the macOS hang
/// `tests/elle/posix.lisp` test #1 was timing out on.
///
/// `SignalReceiver::new` installs a process-wide no-op sigaction
/// handler for each watched signal at refcount 0 → 1 (and restores at
/// 1 → 0) so that the delivery the kernel makes to this worker is a
/// harmless return-through-the-trampoline rather than the default
/// disposition (Term for SIGUSR1, etc.).
#[cfg(target_os = "macos")]
pub(super) fn kq_sig_read_blocking(
    trace: &crate::config::TraceCell,
    kq: RawFd,
    signals: &[libc::c_int],
) -> (i32, Vec<u8>) {
    use crate::io::sigfd::posix_trace;
    posix_trace(
        trace,
        format_args!(
            "macos: kq_sig_read_blocking entered kq={} signals={:?}",
            kq, signals
        ),
    );
    // Unblock the watched signals on this thread for the lifetime of the
    // worker. The worker is single-use (each PoolOp spawns a fresh
    // thread that exits after sending the completion), so we don't
    // bother restoring on return.
    let mut to_unblock: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut to_unblock) };
    for &s in signals {
        unsafe { libc::sigaddset(&mut to_unblock, s) };
    }
    let unblock_ret =
        unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &to_unblock, std::ptr::null_mut()) };
    posix_trace(
        trace,
        format_args!(
            "macos: kq_sig_read_blocking pthread_sigmask SIG_UNBLOCK ret={}",
            unblock_ret
        ),
    );

    loop {
        let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
        posix_trace(
            trace,
            format_args!("macos: kq_sig_read_blocking calling kevent(kq={})", kq),
        );
        let n = unsafe {
            libc::kevent(
                kq,
                std::ptr::null(),
                0,
                eventlist.as_mut_ptr(),
                eventlist.len() as i32,
                std::ptr::null(),
            )
        };
        if n < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            posix_trace(
                trace,
                format_args!(
                    "macos: kq_sig_read_blocking kevent returned -1, errno={}",
                    errno
                ),
            );
            // If the no-op handler ran without SA_RESTART (or some other
            // signal interrupted kevent), retry. The knote state is
            // preserved across EINTR, so a subsequent kevent picks up
            // the same event.
            if errno == libc::EINTR {
                continue;
            }
            return (-errno, Vec::new());
        }
        posix_trace(
            trace,
            format_args!("macos: kq_sig_read_blocking kevent returned n={} events", n),
        );
        let mut data = Vec::with_capacity(n as usize * 8);
        for event in &eventlist[..n as usize] {
            let signum = event.ident as i32;
            let count = event.data as u32;
            posix_trace(
                trace,
                format_args!(
                    "macos: kq_sig_read_blocking event signum={} count={}",
                    signum, count
                ),
            );
            data.extend_from_slice(&signum.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
        }
        return (data.len() as i32, data);
    }
}

/// Blocking kevent() on a kqueue fd (macOS). Encodes results as
/// (fd:i32, fflags:u32) LE pairs for FsWatcher::parse_events().
#[cfg(target_os = "macos")]
pub(super) fn watch_read_blocking(kq: RawFd) -> (i32, Vec<u8>) {
    let mut eventlist: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
    let n = unsafe {
        libc::kevent(
            kq,
            std::ptr::null(),
            0,
            eventlist.as_mut_ptr(),
            eventlist.len() as i32,
            std::ptr::null(),
        )
    };
    if n < 0 {
        return (
            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(1)),
            Vec::new(),
        );
    }
    let mut data = Vec::with_capacity(n as usize * 8);
    for event in &eventlist[..n as usize] {
        data.extend_from_slice(&(event.ident as i32).to_le_bytes());
        data.extend_from_slice(&event.fflags.to_le_bytes());
    }
    (data.len() as i32, data)
}
