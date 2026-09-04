//! Thread-pool backend and stdin thread for async I/O.

use crate::io::grapheme_count_in_valid_prefix;
use crate::io::pending::OpKind;
use crate::io::request::SocketOptions;
use crate::io::SubmissionId;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::time::Duration;

/// Typed thread-pool operation (replaces `op_kind: u8` + overloaded `data`/`size`/`fd`).
///
/// A variant carries only what its syscall needs. How long the operation may
/// wait, and how `io/cancel` ends it, are not a variant's business: they arrive
/// alongside as [`Bounds`], which every `CompletionHub::submit` demands. That
/// is what makes an operation that parks without a bound unwritable rather than
/// merely discouraged.
pub(super) enum PoolOp {
    /// Read up to `size` bytes.
    Read {
        fd: RawFd,
        size: usize,
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
        /// What the port is already holding toward this read — bytes an
        /// earlier read took from the kernel and did not answer with. They
        /// count toward `size`, so the wire is short of the caller's count by
        /// exactly this much and a worker that asked for all of it would wait
        /// for bytes the peer has already sent. The ring counts the same
        /// remainder in its resubmit test (`uring/drain.rs`).
        ///
        /// The bytes rather than their length, because a text `ReadExact`
        /// counts grapheme clusters and one can straddle the boundary between
        /// the remainder and what this read returns.
        held: Vec<u8>,
    },
    /// Write every byte of `data`, looping over short writes.
    Write {
        fd: RawFd,
        data: Vec<u8>,
    },
    Flush {
        fd: RawFd,
    },
    /// Take one connection from a listener.
    Accept {
        fd: RawFd,
    },
    /// Connect to `addr`. The worker opens the socket itself, so the descriptor
    /// it reports back is the connection.
    ConnectTcp {
        addr: std::net::SocketAddr,
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
    /// Take one datagram.
    RecvFrom {
        fd: RawFd,
        size: usize,
    },
    Shutdown {
        fd: RawFd,
        how: i32,
    },
    /// Wait out the bound's own timeout, or until stopped. The duration is the
    /// bound, so a timer has nothing else to carry.
    Sleep,
    /// Reap a child. The worker asks with `WNOHANG` and waits between asks, so
    /// `io/cancel` reaches it and a child that never exits costs no thread past
    /// the fiber that wanted it. `exit` is the handle's record: the ask goes
    /// through it, so a reap this operation's cancellation discards is still
    /// there for the next waiter.
    ProcessWait {
        pid: u32,
        exit: crate::io::request::ExitRecord,
    },
    /// Open a file. Returns the fd (>= 0) on success, or -errno on failure.
    /// O_CLOEXEC is included in `flags` by the primitive — no post-hoc fcntl
    /// needed. The worker adds `O_NONBLOCK` so the open reports rather than
    /// parks, and restores the caller's flags on the descriptor it hands back.
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
    },
    /// Read until EOF. Loops internally, accumulating all data.
    ReadAll {
        fd: RawFd,
    },
    /// Read one batch of filesystem watch events from an inotify (Linux) or
    /// kqueue (macOS) descriptor.
    WatchRead {
        fd: RawFd,
    },
    /// Read one batch of POSIX signal deliveries from a signalfd (Linux).
    /// On macOS the corresponding op is `KqSigRead`.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SigfdRead {
        fd: RawFd,
        /// The watching receiver's instance trace cell, carried onto the worker
        /// thread so its `posix_trace` diagnostics gate per-instance.
        trace: crate::config::TraceCell,
    },
    /// Read one batch of POSIX signal deliveries from a kqueue fd registered
    /// with EVFILT_SIGNAL (macOS). On Linux the corresponding op is
    /// `SigfdRead`.
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
    /// Wait for a raw fd to report readiness. Returns the revents mask.
    PollFd {
        fd: RawFd,
        events: u32,
    },
}

impl PoolOp {
    /// What this operation is, in the terms a completion reports it in.
    ///
    /// The worker knows what it ran; the submission table claims what is in
    /// flight under the id. `PendingOp::accepts` compares the two, so a
    /// completion resolving to the wrong entry is reported rather than cooked
    /// through an arm its payload does not fit. See [`OpKind`].
    pub(super) fn kind(&self) -> OpKind {
        match self {
            PoolOp::Read { .. }
            | PoolOp::ReadExact { .. }
            | PoolOp::ReadLine { .. }
            | PoolOp::ReadAll { .. }
            | PoolOp::Write { .. }
            | PoolOp::Flush { .. }
            | PoolOp::Accept { .. }
            | PoolOp::SendTo { .. }
            | PoolOp::RecvFrom { .. }
            | PoolOp::Shutdown { .. } => OpKind::Port,
            PoolOp::ConnectTcp { .. } | PoolOp::ConnectUnix { .. } => OpKind::Connect,
            PoolOp::Sleep => OpKind::Sleep,
            PoolOp::ProcessWait { .. } => OpKind::ProcessWait,
            PoolOp::Open { .. } => OpKind::Open,
            PoolOp::Task(_) => OpKind::Task,
            PoolOp::Resolve { .. } => OpKind::Resolve,
            PoolOp::WatchRead { .. } => OpKind::Watch,
            PoolOp::SigfdRead { .. } | PoolOp::KqSigRead { .. } => OpKind::Signal,
            PoolOp::PollFd { .. } => OpKind::Poll,
        }
    }
}

/// Typed thread-pool completion (replaces `(u64, i32, Vec<u8>)` tuples).
pub(super) struct PoolCompletion {
    pub(super) id: u64,
    /// What the worker ran, checked against the entry the id resolves through.
    pub(super) kind: OpKind,
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

mod opbound;
/// The declared half of an operation's bound travels out to every submit site,
/// which is where the choice between the three kinds is made.
pub(super) use opbound::Bounds;
use opbound::*;

// `submitop` is the frame every operation shares — hand over, run, publish.
// The rest are the runners it dispatches to, grouped by what they wait on.
mod submitop;

mod pool;
/// The wait a backend that named no keepalive of its own takes, so a test can
/// tell "the default" from a value a caller asked for.
#[cfg(test)]
pub(in crate::io) use pool::DEFAULT_KEEPALIVE;
use pool::{Job, WorkerPool};

mod child;
mod event;
mod net;
mod open;
mod stream;

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
    /// site. A finished operation counts here until its completion is taken. A
    /// cancelled op reports completion like any other and decrements here;
    /// `io/cancel` marks the id and must not also touch this counter.
    ///
    /// Two readers, and neither caps anything: `AsyncBackend::wait` asks whether
    /// there is any worker out before it blocks on the channel, and `io/workers`
    /// reports it. Concurrency is uncapped by design — the pool starts a worker
    /// whenever none is free and lets the OS say how many may run (see
    /// `WorkerPool`), so there is no cap for this count to enforce.
    in_flight: usize,
    /// Linux/uring bridge fd. `None` on the pool-only platforms, where the hub
    /// channel is itself the sole waitable. When `Some`, a worker writes it
    /// after `send` so the ring's single wait observes the edge.
    eventfd: Option<RawFd>,
    /// The write end of every submitted operation's stop pipe, by id. A worker
    /// polls the read end alongside its own descriptor, so a byte written here
    /// ends the operation without disturbing the descriptor — which a port the
    /// caller still holds would not survive.
    stops: HashMap<u64, RawFd>,
    /// The worker threads the operations run on. A finished worker parks here
    /// for the next submission instead of ending, and the pool starts a thread
    /// only when none is parked.
    pool: WorkerPool,
}

impl CompletionHub {
    /// A hub whose workers wait `DEFAULT_KEEPALIVE` for another job. This is
    /// what a backend that was given no keepalive of its own takes.
    pub(super) fn new() -> Self {
        Self::with_keepalive(pool::DEFAULT_KEEPALIVE)
    }

    /// A hub whose workers retire after `keepalive` without a job — what the
    /// program asked for through `*io-keepalive*`, and what a test that wants
    /// to watch a worker retire names rather than sitting out the default.
    pub(super) fn with_keepalive(keepalive: Duration) -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        CompletionHub {
            sender,
            receiver,
            in_flight: 0,
            eventfd: None,
            stops: HashMap::new(),
            pool: WorkerPool::new(keepalive),
        }
    }

    /// The bound for an operation that can wait for something that may never
    /// happen: the caller's deadline, plus a fresh stop pipe. The read end goes
    /// to the worker inside the `Bounds`, which owns it for the operation's
    /// lifetime; the write end stays here until the completion is reaped.
    ///
    /// When the process is out of descriptors there is no stop pipe, and the
    /// operation runs uncancellable — still bounded by the caller's `:timeout`,
    /// which the same wait enforces.
    pub(super) fn bounds(&mut self, id: SubmissionId, timeout: Option<Duration>) -> Bounds {
        let stop = opbound::open_stop_pipe().map(|pipe| {
            self.stops.insert(id.as_u64(), pipe.write_fd);
            pipe.read_fd
        });
        Bounds::new(timeout, stop)
    }

    /// Ask an operation to stop. A second byte would say nothing the first has
    /// not, so a full pipe is success.
    pub(super) fn stop(&mut self, id: SubmissionId) {
        if let Some(&fd) = self.stops.get(&id.as_u64()) {
            let byte = 1u8;
            // SAFETY: `fd` is this hub's write end, closed only by `forget_stop`.
            unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) };
        }
    }

    /// Close an operation's stop pipe once its completion has been reaped.
    pub(super) fn forget_stop(&mut self, id: SubmissionId) {
        if let Some(fd) = self.stops.remove(&id.as_u64()) {
            // SAFETY: the hub owns the write end; the worker owns the read end.
            unsafe { libc::close(fd) };
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

    /// How long this hub's workers wait for another job before retiring. The
    /// test that pins `*io-keepalive*` reaching the crew reads it here.
    #[cfg(test)]
    pub(crate) fn keepalive(&self) -> Duration {
        self.pool.keepalive()
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
        // Every operation still in flight owns the read end of its stop pipe
        // and closes it with itself; the write ends are ours.
        for (_, fd) in self.stops.drain() {
            // SAFETY: the hub is the sole owner of each write end.
            unsafe { libc::close(fd) };
        }
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
