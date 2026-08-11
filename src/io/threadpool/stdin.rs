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
