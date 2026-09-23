//! audited: 2026-09-23
//! The stdin worker: the one thread that reads descriptor 0, and the requests
//! and completions that cross to it.
//!
//! src/io/AGENTS.md
//! docs/io.md

use super::*;

/// Dedicated thread for blocking stdin reads.
///
/// stdin is blocking and cannot go through io_uring without blocking
/// a kernel worker thread. This thread serializes stdin reads, reporting
/// each result to the shared `CompletionHub` as a `RawCompletion::Stdin`
/// (so the scheduler waits on one channel, not a per-source one).
pub(in crate::io) struct StdinThread {
    request_tx: crossbeam_channel::Sender<StdinRequest>,
    /// Write end of the cancellation self-pipe. Writing any byte here
    /// wakes the stdin thread out of `libc::poll` so it can either
    /// (a) acknowledge a shutdown and exit, or (b) treat an in-flight
    /// read as cancelled. Owned by us; closed in `Drop`.
    shutdown_write_fd: RawFd,
    /// Thread handle kept for join in tests and for `is_finished`
    /// observation. In production, the runtime calls `shutdown()` and
    /// then drops the thread; the thread exits within a few syscall
    /// hops of the shutdown write.
    handle: Option<std::thread::JoinHandle<()>>,
}

struct StdinRequest {
    id: u64,
    op_kind: StdinOpKind,
}

pub(in crate::io) enum StdinOpKind {
    ReadLine,
    Read { count: usize },
    ReadAll,
}

pub(in crate::io) struct StdinCompletion {
    pub(in crate::io) id: u64,
    pub(in crate::io) result: Result<Vec<u8>, String>,
}

/// Sentinel string used in the cancelled completion's error message.
/// `(port/close *stdin*)` translates this into an `:io-error` whose
/// `:message` field is exactly `"stdin closed"`, matching the contract
/// documented in `docs/io.md`. Searched for by the threadpool tests.
const STDIN_CLOSED_MSG: &str = "stdin closed";

impl StdinThread {
    /// Spawn the stdin worker. `sender` is a clone of the shared hub channel and
    /// `eventfd` its Linux/uring bridge fd (`None` off uring); the worker reports
    /// each completion via `publish_completion` so it lands on the one channel
    /// the scheduler waits on.
    pub(in crate::io) fn new(
        sender: crossbeam_channel::Sender<RawCompletion>,
        eventfd: Option<RawFd>,
    ) -> Self {
        let (request_tx, request_rx) = crossbeam_channel::unbounded::<StdinRequest>();

        // Self-pipe for cancellation. The thread polls the read end
        // alongside fd 0; writing any byte here wakes the poll(2).
        // We set the read end to O_NONBLOCK so the thread's drain
        // (after a shutdown wakeup) never blocks.
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];
        let pipe_ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        if pipe_ret != 0 {
            panic!(
                "StdinThread: pipe(2) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let shutdown_read_fd = pipe_fds[0];
        let shutdown_write_fd = pipe_fds[1];
        unsafe {
            libc::fcntl(shutdown_read_fd, libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(shutdown_read_fd, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(shutdown_write_fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }

        let handle = std::thread::Builder::new()
            .name("elle-stdin".into())
            .spawn(move || {
                crate::io::sigfd::mask_all_signals_on_this_thread();
                stdin_thread_loop(request_rx, sender, eventfd, shutdown_read_fd);
                unsafe { libc::close(shutdown_read_fd) };
            })
            .expect("failed to spawn stdin thread");

        StdinThread {
            request_tx,
            shutdown_write_fd,
            handle: Some(handle),
        }
    }

    pub(in crate::io) fn submit(
        &self,
        id: SubmissionId,
        op_kind: StdinOpKind,
    ) -> Result<(), String> {
        let id = id.as_u64();
        self.request_tx
            .send(StdinRequest { id, op_kind })
            .map_err(|_| "stdin thread channel disconnected".to_string())
    }

    /// Signal the stdin thread to shut down. The thread either:
    ///   - if currently inside `poll(2)` waiting for input on fd 0,
    ///     observes the shutdown pipe revents and sends a `stdin
    ///     closed` error completion for the in-flight request before
    ///     exiting;
    ///   - if currently waiting in `request_rx.recv_timeout`, picks
    ///     the shutdown up on its next 100 ms tick and exits.
    ///
    /// Idempotent: subsequent calls write extra bytes into the pipe
    /// which the thread either drains on exit or never reads (already
    /// gone). The write is bounded to 1 byte so it cannot ever
    /// block on a full kernel pipe buffer.
    pub(in crate::io) fn shutdown(&self) {
        let byte: u8 = 1;
        unsafe {
            libc::write(
                self.shutdown_write_fd,
                &byte as *const u8 as *const libc::c_void,
                1,
            );
        }
    }

    /// True once the worker thread has exited. Used by tests to assert
    /// `shutdown()` actually wound the thread down; callers in the
    /// runtime don't need this (the drop path waits for them).
    #[allow(dead_code)]
    pub(in crate::io) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
}

impl Drop for StdinThread {
    fn drop(&mut self) {
        // Signal shutdown so the worker exits promptly. Closing the
        // write end signals EOF on the pipe — the thread's poll picks
        // it up too — but `shutdown()` writes a byte first to wake
        // any current poll. Either is sufficient; both is robust.
        self.shutdown();
        unsafe { libc::close(self.shutdown_write_fd) };
        if let Some(h) = self.handle.take() {
            // Best-effort join. The thread is bounded by the next poll
            // tick (~100 ms) plus the time to send any pending
            // cancellation completion. In practice this returns
            // quickly; we tolerate a brief blip on Drop rather than
            // detaching and leaking a thread.
            let _ = h.join();
        }
    }
}

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
fn stdin_thread_loop(
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
fn shutdown_signalled(shutdown_read_fd: RawFd) -> bool {
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
fn poll_stdin_or_shutdown(shutdown_read_fd: RawFd) -> Result<bool, String> {
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
fn read_chunk_with_cancel(shutdown_read_fd: RawFd, max: usize) -> Result<Option<Vec<u8>>, String> {
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

fn read_line_with_cancel(
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

fn read_n_with_cancel(
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

fn read_all_with_cancel(
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
