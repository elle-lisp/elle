//! Thread-pool backend and stdin thread for async I/O.

use crate::io::grapheme_count_in_valid_prefix;
use crate::io::request::SocketOptions;
use crate::io::SubmissionId;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::time::Duration;

/// Typed thread-pool operation (replaces `op_kind: u8` + overloaded `data`/`size`/`fd`).
pub(super) enum PoolOp {
    /// Read up to `size` bytes. `timeout` bounds the wait for data to arrive,
    /// via the fd's own receive timeout; `None` waits indefinitely. Every read
    /// variant carries it — these worker fds are blocking, so without it a
    /// peer that goes quiet parks the worker forever.
    Read {
        fd: RawFd,
        size: usize,
        timeout: Option<Duration>,
    },
    /// Read exactly `size` units, looping until full or EOF/error.
    /// Units are bytes when `graphemes` is false and grapheme clusters
    /// when true — Elle strings are grapheme-counted, so a text-port
    /// `port/read-exact 50` must yield a string of `(length 50)`
    /// regardless of how many kernel bytes that took.  The worker
    /// keeps calling `read(2)` until the requested count is met,
    /// the peer closes, or an error fires.  On EOF before `size`,
    /// the completion path treats the partial result as nil.
    ReadExact {
        fd: RawFd,
        size: usize,
        graphemes: bool,
        /// The generation that segments cluster-counted reads; captured at
        /// request build on the VM thread, applied on the worker thread.
        gen: crate::segment::Generation,
        timeout: Option<Duration>,
    },
    /// Write every byte of `data`, looping over short writes. `timeout`
    /// bounds the wait for the fd to become writable on each pass, so a peer
    /// that stops reading cannot hang the write past the caller's deadline;
    /// `None` waits indefinitely.
    Write {
        fd: RawFd,
        data: Vec<u8>,
        timeout: Option<Duration>,
    },
    Flush {
        fd: RawFd,
    },
    Accept {
        fd: RawFd,
    },
    ConnectTcp {
        addr: String,
        options: SocketOptions,
    },
    ConnectUnix {
        path: String,
        options: SocketOptions,
    },
    SendTo {
        fd: RawFd,
        addr: String,
        port: u16,
        data: Vec<u8>,
    },
    RecvFrom {
        fd: RawFd,
        size: usize,
    },
    Shutdown {
        fd: RawFd,
        how: i32,
    },
    Sleep {
        nanos: u64,
    },
    ProcessWait {
        pid: u32,
    },
    /// Open a file asynchronously. Returns the fd (>= 0) on success, or -errno on failure.
    /// O_CLOEXEC is included in `flags` by the primitive — no post-hoc fcntl needed.
    Open {
        path: std::ffi::CString,
        flags: i32,
        mode: u32,
    },
    /// Run an arbitrary closure. Returns (result_code, data).
    Task(Box<dyn FnOnce() -> (i32, Vec<u8>) + Send>),
    /// Resolve a hostname via getaddrinfo(3). Returns IP addresses as
    /// newline-separated strings in `data`, result_code 0 on success.
    Resolve {
        hostname: String,
    },
    /// Read until a newline is found or EOF. Loops internally so the caller
    /// always receives data containing `\n` (or the final chunk at EOF).
    ReadLine {
        fd: RawFd,
        timeout: Option<Duration>,
    },
    /// Read until EOF. Loops internally, accumulating all data.
    ReadAll {
        fd: RawFd,
        timeout: Option<Duration>,
    },
    /// Blocking read on an inotify/kqueue fd for filesystem watch events.
    WatchRead {
        fd: RawFd,
    },
    /// Blocking read on a signalfd (Linux) for POSIX signal deliveries.
    /// On macOS the corresponding op is `KqSigRead`.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SigfdRead {
        fd: RawFd,
        /// The watching receiver's instance trace cell, carried onto the worker
        /// thread so its `posix_trace` diagnostics gate per-instance.
        trace: crate::config::TraceCell,
    },
    /// Blocking kevent() on a kqueue fd registered with EVFILT_SIGNAL (macOS).
    /// On Linux the corresponding op is `SigfdRead`.
    ///
    /// `signals` is the set the watcher is interested in. The worker
    /// unblocks them on its own thread before calling kevent() because
    /// kqueue's `EVFILT_SIGNAL` fires from the in-kernel delivery path
    /// — when every thread in the process blocks the signal the kernel
    /// parks it on the process pending list and the knote never
    /// activates (no thread is selected for delivery, so the kqueue
    /// hook in psignal_internal is never reached). `SignalReceiver::new`
    /// installs a process-wide no-op sigaction handler so the signal
    /// delivered to this thread does no harm.
    #[cfg_attr(any(target_os = "linux", target_os = "android"), allow(dead_code))]
    KqSigRead {
        fd: RawFd,
        signals: Vec<libc::c_int>,
        /// The watching receiver's instance trace cell, carried onto the worker
        /// thread so its `posix_trace` diagnostics gate per-instance.
        trace: crate::config::TraceCell,
    },
    /// Poll a raw fd for readiness via libc::poll(). Returns revents mask.
    PollFd {
        fd: RawFd,
        events: u32,
        timeout_ms: i32,
    },
}

/// Typed thread-pool completion (replaces `(u64, i32, Vec<u8>)` tuples).
pub(super) struct PoolCompletion {
    pub(super) id: u64,
    pub(super) result_code: i32,
    pub(super) data: Vec<u8>,
}

/// A completion from a background worker, before cooking into a `Completion`.
///
/// Workers run off the main thread and can't build cooked `Completion`s — the
/// cook fns need main-thread `pending`/`fd_states`/`buffer_pool`/`origin_heap`.
/// So every worker (the thread-pool workers and the stdin worker) ships its raw
/// result through the one shared hub channel as a `RawCompletion`; the receiver
/// matches once and dispatches to `pool_to_completion` / `stdin_to_completion`.
pub(super) enum RawCompletion {
    Pool(PoolCompletion),
    Stdin(StdinCompletion),
}

/// Maximum concurrent thread-pool operations.
pub(super) const MAX_THREAD_POOL_OPS: usize = 64;

mod submitop;

/// The single completion channel every background worker feeds.
///
/// Collapsing the former platform-pool, network-pool, and stdin channels into
/// one means the scheduler's blocking wait reads exactly one source: a crossbeam
/// `recv()` registers-before-sleeps on the sole channel, so there is nothing to
/// exclude and no wakeup to miss (the lost-wakeup fix by construction).
pub(super) struct CompletionHub {
    sender: crossbeam_channel::Sender<RawCompletion>,
    receiver: crossbeam_channel::Receiver<RawCompletion>,
    /// Combined count of submitted-but-unreaped worker ops (pool + stdin): +1
    /// per worker submit, −1 once per `RawCompletion` reaped at the single drain
    /// site. Read only to decide whether the scheduler should block at all. A
    /// cancelled op's reaped completion still decrements here; `io/cancel` only
    /// removes the `pending` entry and must not also touch this counter.
    in_flight: usize,
    /// Linux/uring bridge fd. `None` on the pool-only platforms, where the hub
    /// channel is itself the sole waitable. When `Some`, a worker writes it
    /// after `send` so the ring's single wait observes the edge.
    eventfd: Option<RawFd>,
}

impl CompletionHub {
    pub(super) fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        CompletionHub {
            sender,
            receiver,
            in_flight: 0,
            eventfd: None,
        }
    }

    /// A `Sender<RawCompletion>` clone for a worker (pool or stdin).
    pub(super) fn sender(&self) -> crossbeam_channel::Sender<RawCompletion> {
        self.sender.clone()
    }

    /// The bridge eventfd, if this hub is wired to a ring.
    pub(super) fn eventfd(&self) -> Option<RawFd> {
        self.eventfd
    }

    /// Attach the Linux/uring bridge eventfd. Called once at backend
    /// construction on the uring platform; the hub then owns the fd (closed in
    /// its `Drop`) and every worker raises it after `send`.
    #[cfg(target_os = "linux")]
    pub(super) fn set_eventfd(&mut self, fd: RawFd) {
        self.eventfd = Some(fd);
    }

    /// True when any pool/stdin op is submitted-but-unreaped.
    pub(super) fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Account one submitted worker op (a pool submit or a stdin request).
    pub(super) fn note_submit(&mut self) {
        self.in_flight += 1;
    }

    /// Drain every completion ready now, decrementing the counter once each.
    /// Saturating so a stray completion (e.g. the stdin worker's cancel-drain)
    /// can never underflow the count.
    pub(super) fn drain_raw(&mut self) -> Vec<RawCompletion> {
        let mut out = Vec::new();
        while let Ok(rc) = self.receiver.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            out.push(rc);
        }
        out
    }

    /// Block for one completion — the sole register-before-sleep wait on the
    /// pool platform. `None` blocks forever; `Some(d)` bounds the wait; a
    /// timeout (or disconnect) returns `None`. Decrements the counter once for
    /// the returned item.
    pub(super) fn recv_blocking(&mut self, timeout: Option<Duration>) -> Option<RawCompletion> {
        let rc = match timeout {
            None => self.receiver.recv().ok(),
            Some(d) => self.receiver.recv_timeout(d).ok(),
        }?;
        self.in_flight = self.in_flight.saturating_sub(1);
        Some(rc)
    }
}

impl Drop for CompletionHub {
    fn drop(&mut self) {
        // The hub owns the bridge eventfd (Linux/uring); close it on teardown.
        // The backend's `AsyncBackendInner` declares `platform` before `hub`, so
        // the ring (and the standing POLL_ADD referencing this fd) is already
        // torn down by the time we get here. A detached pool worker may still
        // hold the raw value and `signal` it after this close — that write hits
        // a closed fd (EBADF), which is benign for the wake protocol.
        #[cfg(target_os = "linux")]
        if let Some(fd) = self.eventfd.take() {
            // SAFETY: the hub is the sole owner of this fd's lifetime.
            unsafe { libc::close(fd) };
        }
    }
}

#[cfg(test)]
impl CompletionHub {
    /// Block up to `timeout_ms` for pool completions, draining any that arrived
    /// alongside, and return their `PoolCompletion`s. Panics on a stdin
    /// completion — the worker-level tests (ProcessWait / Open / signal reads)
    /// submit only pool ops. Returns a `Result` (always `Ok` while the hub is
    /// alive) so those tests can keep their `?`/`match` on the submit-then-wait
    /// shape.
    pub(super) fn wait_pool(
        &mut self,
        timeout_ms: Option<u64>,
    ) -> Result<Vec<PoolCompletion>, String> {
        let mut raw = Vec::new();
        if let Some(rc) = self.recv_blocking(timeout_ms.map(Duration::from_millis)) {
            raw.push(rc);
            raw.extend(self.drain_raw());
        }
        Ok(raw
            .into_iter()
            .map(|rc| match rc {
                RawCompletion::Pool(pc) => pc,
                RawCompletion::Stdin(_) => panic!("wait_pool: unexpected stdin completion"),
            })
            .collect())
    }
}

/// Publish a worker completion: send it on the hub channel, then — on the
/// Linux/uring bridge only — raise the eventfd edge. The order is load-bearing:
/// publish the item *before* raising the edge, or a wake could drain-empty,
/// re-arm, re-block, and miss the just-sent item.
pub(super) fn publish_completion(
    sender: &crossbeam_channel::Sender<RawCompletion>,
    eventfd: Option<RawFd>,
    rc: RawCompletion,
) {
    let _ = sender.send(rc);
    // Raise the bridge edge after the item is published so the ring-side poll,
    // once woken, always sees it. The eventfd is `Some` only on Linux/uring; on
    // the pool-only platforms the channel is the sole waitable and this is a
    // no-op. `crate::io::eventfd` is Linux-gated, so the call is cfg-gated too.
    #[cfg(target_os = "linux")]
    if let Some(efd) = eventfd {
        crate::io::eventfd::signal(efd);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = eventfd;
}

// --- StdinThread ---

/// Dedicated thread for blocking stdin reads.
///
/// stdin is blocking and cannot go through io_uring without blocking
/// a kernel worker thread. This thread serializes stdin reads, reporting
/// each result to the shared `CompletionHub` as a `RawCompletion::Stdin`
/// (so the scheduler waits on one channel, not a per-source one).
pub(super) struct StdinThread {
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

pub(super) struct StdinRequest {
    id: u64,
    op_kind: StdinOpKind,
}

pub(super) enum StdinOpKind {
    ReadLine,
    Read { count: usize },
    ReadAll,
}

pub(super) struct StdinCompletion {
    pub(super) id: u64,
    pub(super) result: Result<Vec<u8>, String>,
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
    pub(super) fn new(
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

    pub(super) fn submit(&self, id: SubmissionId, op_kind: StdinOpKind) -> Result<(), String> {
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
    pub(super) fn shutdown(&self) {
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
    pub(super) fn is_finished(&self) -> bool {
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

mod stdin;
use stdin::*;

#[cfg(test)]
mod tests;
