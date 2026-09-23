//! audited: 2026-09-23
//! `CompletionHub`: the channel every worker reports through, the stop pipes
//! that end its operations, and its crew.
//!
//! src/io/AGENTS.md
//! docs/impl/io-descriptor.md

use super::*;

/// The single completion channel every background worker feeds.
///
/// The pool workers and the stdin worker all send here, so the scheduler's
/// blocking wait reads exactly one source: a crossbeam `recv()` registers
/// before it sleeps on the sole channel, so there is no wakeup to miss.
pub(in crate::io) struct CompletionHub {
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
    pub(super) pool: WorkerPool,
}

impl CompletionHub {
    /// A hub whose workers wait `DEFAULT_KEEPALIVE` for another job. This is
    /// what a backend that was given no keepalive of its own takes.
    pub(in crate::io) fn new() -> Self {
        Self::with_keepalive(super::pool::DEFAULT_KEEPALIVE)
    }

    /// A hub whose workers retire after `keepalive` without a job — what the
    /// program asked for through `*io-keepalive*`, and what a test that wants
    /// to watch a worker retire names rather than sitting out the default.
    pub(in crate::io) fn with_keepalive(keepalive: Duration) -> Self {
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
    pub(in crate::io) fn bounds(&mut self, id: SubmissionId, timeout: Option<Duration>) -> Bounds {
        let stop = super::opbound::open_stop_pipe().map(|pipe| {
            self.stops.insert(id.as_u64(), pipe.write_fd);
            pipe.read_fd
        });
        Bounds::new(timeout, stop)
    }

    /// Ask an operation to stop. A second byte would say nothing the first has
    /// not, so a full pipe is success.
    pub(in crate::io) fn stop(&mut self, id: SubmissionId) {
        if let Some(&fd) = self.stops.get(&id.as_u64()) {
            let byte = 1u8;
            // SAFETY: `fd` is this hub's write end, closed only by `forget_stop`.
            unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) };
        }
    }

    /// Every submitted operation that carries a stop pipe: the ones a stop
    /// ends at once, which a backend coming to rest stops and waits for.
    pub(in crate::io) fn stoppable(&self) -> Vec<SubmissionId> {
        self.stops
            .keys()
            .map(|&id| SubmissionId::from_raw(id))
            .collect()
    }

    /// Close an operation's stop pipe once its completion has been reaped.
    pub(in crate::io) fn forget_stop(&mut self, id: SubmissionId) {
        if let Some(fd) = self.stops.remove(&id.as_u64()) {
            // SAFETY: the hub owns the write end; the worker owns the read end.
            unsafe { libc::close(fd) };
        }
    }

    /// A `Sender<RawCompletion>` clone for a worker (pool or stdin).
    pub(in crate::io) fn sender(&self) -> crossbeam_channel::Sender<RawCompletion> {
        self.sender.clone()
    }

    /// The bridge eventfd, if this hub is wired to a ring.
    pub(in crate::io) fn eventfd(&self) -> Option<RawFd> {
        self.eventfd
    }

    /// Attach the Linux/uring bridge eventfd. Called once at backend
    /// construction on the uring platform; the hub then owns the fd (closed in
    /// its `Drop`) and every worker raises it after `send`.
    #[cfg(target_os = "linux")]
    pub(in crate::io) fn set_eventfd(&mut self, fd: RawFd) {
        self.eventfd = Some(fd);
    }

    /// True when any pool/stdin op is submitted-but-unreaped.
    pub(in crate::io) fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// How long this hub's workers wait for another job before retiring. The
    /// test that pins `*io-keepalive*` reaching the crew reads it here.
    #[cfg(test)]
    pub(crate) fn keepalive(&self) -> Duration {
        self.pool.keepalive()
    }

    /// Account one submitted worker op (a pool submit or a stdin request).
    pub(in crate::io) fn note_submit(&mut self) {
        self.in_flight += 1;
    }

    /// Drain every completion ready now, decrementing the counter once each.
    /// Saturating so a stray completion (e.g. the stdin worker's cancel-drain)
    /// can never underflow the count.
    pub(in crate::io) fn drain_raw(&mut self) -> Vec<RawCompletion> {
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
    pub(in crate::io) fn recv_blocking(
        &mut self,
        timeout: Option<Duration>,
    ) -> Option<RawCompletion> {
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
    pub(in crate::io) fn wait_pool(
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
/// Linux/uring bridge only — raise the eventfd edge. The order matters:
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
