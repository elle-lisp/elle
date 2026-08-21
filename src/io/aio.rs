//! AsyncBackend — asynchronous I/O backend.
//!
//! Uses io_uring on Linux (feature-gated), thread-pool fallback elsewhere.

use crate::io::completion;
use crate::io::pending::PendingOp;
use crate::io::pool::BufferPool;
use crate::io::request::{
    ConnectAddr, IoOp, IoRequest, PortOp, ProcessHandle, ProcessState, SpawnRequest, TaskFn,
};
use crate::io::threadpool::{
    Bounds, CompletionHub, PoolCompletion, PoolOp, RawCompletion, StdinOpKind, StdinThread,
};
use crate::io::types::{FdState, PortKey};
use crate::io::{Completion, SubmissionId};
use crate::port::{Encoding, Port, PortKind};
use crate::value::Value;

use std::cell::RefCell;

use convert::cook_raw;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

/// Async I/O backend. Wrapped as ExternalObject "io-backend".
pub struct AsyncBackend {
    inner: RefCell<AsyncBackendInner>,
}

struct AsyncBackendInner {
    /// The owning VM's Unicode generation, captured at backend construction.
    /// Grapheme-counted text reads (`read-exact` on a text port) split the
    /// byte stream at cluster boundaries with it, on and off the VM thread.
    unicode_generation: crate::segment::Generation,
    fd_states: HashMap<PortKey, FdState>,
    pending: HashMap<SubmissionId, PendingOp>,
    /// Submissions whose result no fiber will receive. The `pending` entry
    /// stays, so the worker's completion is still matched, counted and
    /// released; only the cooked value is dropped. Dropping the entry instead
    /// would strand the submission — the worker it runs on and the descriptor
    /// it names would never come back. See src/io/AGENTS.md
    /// § "I/O Cancellation".
    cancelled: std::collections::HashSet<SubmissionId>,
    /// Descriptors kept open past their port's close because a submitted
    /// operation still names them. A worker resolves its fd when it runs, so a
    /// number handed back to the OS while an operation holds it can be given
    /// to a new socket that the stale operation then reads.
    retired: HashMap<RawFd, std::os::unix::io::OwnedFd>,
    completions: VecDeque<Completion>,
    next_id: u64,
    // `platform` is declared before `buffer_pool` so it drops first: tearing
    // the io_uring ring down (closing its fd, which makes the kernel cancel and
    // finish in-flight ops) before the pool frees is a second line of defence
    // behind `quiesce_pending`, so a kernel write can never land in a freed
    // pool slot. See `Drop for AsyncBackend` and docs/io.md "Backend teardown".
    platform: PlatformBackend,
    buffer_pool: BufferPool,
    stdin_thread: Option<StdinThread>,
    /// The one completion channel every background worker feeds — the thread
    /// pool (everything that can't lift to io_uring: getaddrinfo, `Task`, and
    /// all I/O on the pool platform) and the stdin worker. The scheduler's
    /// blocking wait reads exactly this one source.
    hub: CompletionHub,
    /// The requesting instance's heap, captured from `submit`'s `origin_heap`.
    /// A backend serves exactly one instance (it is created per scheduler), so
    /// this is constant once set; every completion the scheduler-thread harvest
    /// builds is born on it (`crate::io::completion_heap_ptr`). Set on the first
    /// `submit` that carries a heap.
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
}

// --- Platform backend dispatch ---

pub(crate) enum PlatformBackend {
    #[cfg(target_os = "linux")]
    Uring(Box<io_uring::IoUring>),
    /// The pool platform (macOS, or Linux `--no-uring`). There is no separate
    /// pool object — all pool work runs through the shared `CompletionHub`; this
    /// variant only marks which `wait()` path the scheduler takes.
    ThreadPool,
}

/// High bit tag for timeout CQE user_data.
#[cfg(target_os = "linux")]
pub(crate) const TIMEOUT_USER_DATA_TAG: u64 = 1 << 63;

/// Sentinel `user_data` for the standing eventfd `POLL_ADD` that bridges hub
/// completions into the io_uring wait. Distinct by construction from every
/// minted `SubmissionId` (those count up from 1; `mint_id` asserts it never
/// reaches this) and from `TIMEOUT_USER_DATA_TAG` (the high bit — this value's
/// high bit is clear, so it is never mistaken for a timeout CQE). `drain_cqes`
/// matches it before the timeout tag and the `pending` lookup.
#[cfg(target_os = "linux")]
pub(crate) const EVENTFD_USER_DATA: u64 = u64::MAX >> 1;

impl std::fmt::Debug for AsyncBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<io-backend:async>")
    }
}

impl AsyncBackend {
    /// Create a new async backend.
    ///
    /// On Linux with the `io-uring` feature, attempts io_uring first.
    /// Falls back to thread-pool on failure or on non-Linux platforms.
    /// Uses the process-default Unicode generation; a backend serving a VM
    /// with an explicit generation is built via [`Self::new_with_unicode`].
    pub fn new() -> Result<Self, String> {
        Self::new_with_unicode(crate::config::get().unicode_generation())
    }

    /// Create a new async backend serving a VM with the given Unicode
    /// generation.
    pub fn new_with_unicode(gen: crate::segment::Generation) -> Result<Self, String> {
        let mut platform = Self::create_platform_backend();
        let mut hub = CompletionHub::new();
        // On the uring platform, wire the eventfd bridge: a hub worker raises
        // the eventfd after publishing, and a standing POLL_ADD on the ring
        // turns that edge into a CQE so the scheduler's single io_uring wait
        // returns. No-op on the pool-only platforms (no ring, no eventfd).
        Self::wire_eventfd_bridge(&mut platform, &mut hub)?;
        Ok(AsyncBackend {
            inner: RefCell::new(AsyncBackendInner {
                unicode_generation: gen,
                fd_states: HashMap::new(),
                pending: HashMap::new(),
                cancelled: std::collections::HashSet::new(),
                retired: HashMap::new(),
                completions: VecDeque::new(),
                next_id: 1,
                buffer_pool: BufferPool::new(),
                stdin_thread: None,
                platform,
                hub,
                origin_heap: std::ptr::null_mut(),
            }),
        })
    }

    /// A backend on the THREAD-POOL platform, whatever this host would pick.
    ///
    /// The pool is what every non-Linux build runs (`create_platform_backend`
    /// has no other arm there) and what a Linux host runs when io_uring is
    /// unavailable or `--no-uring` is set. Its wait path differs from the ring's,
    /// so the properties that hold on one are not evidence about the other. A
    /// test that built the host's default backend would exercise the ring on a
    /// Linux dev box and the pool on CI — silently checking different code on
    /// each, which is how a pool-only defect stays invisible. This constructor
    /// makes the platform an explicit choice of the test rather than a property
    /// of the box it runs on.
    /// No eventfd bridge is wired: the pool platform has no ring to bridge into,
    /// so its hub channel is the sole waitable — the same shape a non-Linux build
    /// comes up with.
    #[cfg(test)]
    pub(crate) fn new_thread_pool() -> Result<Self, String> {
        Ok(AsyncBackend {
            inner: RefCell::new(AsyncBackendInner {
                unicode_generation: crate::config::get().unicode_generation(),
                fd_states: HashMap::new(),
                pending: HashMap::new(),
                cancelled: std::collections::HashSet::new(),
                retired: HashMap::new(),
                completions: VecDeque::new(),
                next_id: 1,
                buffer_pool: BufferPool::new(),
                stdin_thread: None,
                platform: PlatformBackend::ThreadPool,
                hub: CompletionHub::new(),
                origin_heap: std::ptr::null_mut(),
            }),
        })
    }

    /// Create the bridge eventfd, hand it to the hub (which owns and closes it),
    /// and arm the standing `POLL_ADD` on the ring. Only the uring platform has
    /// a ring to bridge into; every other platform leaves the hub eventfd-less
    /// (its channel is itself the sole waitable). A failure here propagates so a
    /// backend can never come up with a half-wired, deaf bridge.
    #[cfg(target_os = "linux")]
    fn wire_eventfd_bridge(
        platform: &mut PlatformBackend,
        hub: &mut CompletionHub,
    ) -> Result<(), String> {
        if let PlatformBackend::Uring(ring) = platform {
            let efd = crate::io::eventfd::create()
                .map_err(|e| format!("io backend: eventfd bridge: {}", e))?;
            // Store before arming: an arm failure then drops the hub on the
            // error path, closing the fd we just created.
            hub.set_eventfd(efd);
            crate::io::uring::arm_eventfd_poll(ring, efd)?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn wire_eventfd_bridge(
        _platform: &mut PlatformBackend,
        _hub: &mut CompletionHub,
    ) -> Result<(), String> {
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn create_platform_backend() -> PlatformBackend {
        if crate::config::get().no_uring {
            return PlatformBackend::ThreadPool;
        }
        match io_uring::IoUring::new(256) {
            Ok(ring) => PlatformBackend::Uring(Box::new(ring)),
            Err(_) => PlatformBackend::ThreadPool,
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn create_platform_backend() -> PlatformBackend {
        PlatformBackend::ThreadPool
    }

    /// Cancel and drain every in-flight io_uring operation so no kernel-owned
    /// buffer outlives this backend. Idempotent — a no-op once `pending` is
    /// empty. Called from `Drop`; also callable directly (tests). See
    /// `AsyncBackendInner::quiesce_pending` and docs/io.md "Backend teardown".
    pub(crate) fn quiesce(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.quiesce_pending();
        }
    }

    /// True when the platform backend is io_uring (vs the thread-pool
    /// fallback). Tests gate uring-specific assertions on this.
    #[cfg(all(target_os = "linux", test))]
    pub(crate) fn is_uring(&self) -> bool {
        matches!(self.inner.borrow().platform, PlatformBackend::Uring(_))
    }

    /// The ids of every operation still in flight, ascending. Tests pin the
    /// submission frame with it: one submission files one pending entry, and
    /// the entry is keyed by the id `submit` returned.
    #[cfg(test)]
    pub(crate) fn pending_ids(&self) -> Vec<SubmissionId> {
        let mut ids: Vec<SubmissionId> = self.inner.borrow().pending.keys().copied().collect();
        ids.sort();
        ids
    }
}

impl Drop for AsyncBackend {
    fn drop(&mut self) {
        // Bring the ring to a quiescent state before its buffer pool and SQ/CQ
        // are freed: any operation still in flight (submitted but never reaped,
        // e.g. `io/submit` with no matching `io/wait`) has the kernel holding a
        // write pointer into a pool/arena buffer. Reaping it here keeps that
        // write from landing in freed heap.
        self.quiesce();
    }
}

mod convert;
mod poll;
mod requests;
mod submit;

impl crate::io::IoBackend for AsyncBackend {
    fn submit(
        &self,
        request: &IoRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<SubmissionId, String> {
        self.submit(request, origin_heap)
    }

    fn poll(&self) -> Vec<Completion> {
        self.poll()
    }

    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String> {
        self.wait(timeout_ms)
    }

    fn workers(&self) -> usize {
        self.workers()
    }

    fn cancel(&self, id: SubmissionId) -> Result<(), String> {
        self.cancel(id)
    }

    fn quiesce(&self) {
        self.quiesce();
    }
}

impl AsyncBackendInner {
    /// Cancel and drain every in-flight io_uring operation so no kernel-owned
    /// buffer outlives the backend.
    ///
    /// An io_uring SQE references a buffer the kernel writes into
    /// asynchronously — a `BufferPool` slot (`read-all`/`open`/`write`) or the
    /// fiber's arena buffer (`read`/`read-line`). If the backend is dropped
    /// with an op still in flight (submitted but never reaped, e.g. `io/submit`
    /// with no `io/wait`), the pool and ring are freed while the kernel still
    /// owns that pointer, and the eventual write lands in freed heap
    /// (`malloc(): unsorted double linked list corrupted`). Cancel each pending
    /// op — a cancelled io_uring read completes with `-ECANCELED` — and drain
    /// the resulting CQEs (which release the buffers) so nothing kernel-owned
    /// survives this call.
    ///
    /// Ops serviced by the stdin/network channels post no CQE here; they make
    /// no progress, so we stop after the first pass that neither shrinks
    /// `pending` nor drains a completion. Those workers copy results through
    /// channels and never write into a freed pooled buffer, so leaving them is
    /// safe. The pass count is bounded as a backstop against an op that keeps
    /// re-submitting (a continuously-readable fd reaped via resubmission).
    #[cfg(target_os = "linux")]
    fn quiesce_pending(&mut self) {
        if !matches!(self.platform, PlatformBackend::Uring(_)) || self.pending.is_empty() {
            return;
        }
        let mut sink: VecDeque<Completion> = VecDeque::new();
        let mut passes = 0u32;
        while !self.pending.is_empty() && passes < 64 {
            passes += 1;
            let before = self.pending.len();
            let ids: Vec<SubmissionId> = self.pending.keys().copied().collect();

            // Cancel everything still pending. A cancel for an id the ring
            // doesn't know (a stdin/network op) returns a tagged `-ENOENT` CQE
            // that `drain_cqes` skips, so it is harmless.
            if let PlatformBackend::Uring(ref mut ring) = self.platform {
                for id in &ids {
                    let _ = crate::io::uring::submit_uring_cancel(ring, *id);
                }
            }

            // Wait briefly for the cancellation CQEs, then drain them. A cancel
            // posts its CQE promptly, so 50ms is a ceiling, not the expected
            // latency.
            let origin_heap = self.origin_heap;
            let gen = self.unicode_generation;
            let AsyncBackendInner {
                ref mut platform,
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ..
            } = *self;
            if let PlatformBackend::Uring(ring) = platform {
                let ts = io_uring::types::Timespec::new().sec(0).nsec(50_000_000);
                let args = io_uring::types::SubmitArgs::new().timespec(&ts);
                match ring.submitter().submit_with_args(1, &args) {
                    Ok(_) => {}
                    Err(e) if e.raw_os_error() == Some(libc::ETIME) => {}
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {}
                    Err(_) => break,
                }
                // Teardown: the ring is about to close, so the standing eventfd
                // POLL_ADD is left to be cancelled with everything else. We do
                // not service it (no re-arm) — a fired sentinel is just skipped.
                let mut bridge_fired = false;
                crate::io::uring::drain_cqes(
                    ring,
                    pending,
                    buffer_pool,
                    fd_states,
                    &mut sink,
                    origin_heap,
                    gen,
                    &mut bridge_fired,
                );
            }

            let drained = !sink.is_empty();
            sink.clear();
            // No CQE and no shrink ⇒ only channel-serviced ops remain; stop.
            if !drained && self.pending.len() >= before {
                break;
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn quiesce_pending(&mut self) {}

    /// Mint the next unique, monotonically increasing submission id.
    fn mint_id(&mut self) -> SubmissionId {
        // The eventfd bridge reserves `EVENTFD_USER_DATA` as a CQE sentinel; a
        // minted id colliding with it would make `drain_cqes` mis-route a real
        // completion as the bridge wake. Unreachable in practice (the counter
        // would need 2^63 submits) — asserted so a future change can't break it.
        #[cfg(target_os = "linux")]
        debug_assert_ne!(
            self.next_id, EVENTFD_USER_DATA,
            "submission id counter reached the eventfd bridge sentinel"
        );
        let id = SubmissionId::from_raw(self.next_id);
        self.next_id += 1;
        id
    }

    /// Drain everything ready now into self.completions: the ring's CQEs (uring
    /// platform) and the shared hub (pool + stdin workers), on both platforms.
    fn drain_ready(&mut self) {
        self.drain_uring_completions();
        self.drain_hub();
    }

    /// Drain the io_uring completion queue into self.completions. A no-op on the
    /// pool platform (no ring); all its work surfaces through `drain_hub`.
    ///
    /// This is the non-blocking drain (poll path, and the pre/post passes of a
    /// blocking wait). If it consumes the standing eventfd bridge `POLL_ADD`
    /// CQE, it clears the eventfd and re-arms the one-shot poll here — otherwise
    /// the next blocking wait would have no armed poll watching the bridge and a
    /// hub worker's wake would be lost.
    fn drain_uring_completions(&mut self) {
        // Only the ring arm below consumes these; the pool platform compiles
        // that arm out and would see three unused bindings.
        #[cfg(target_os = "linux")]
        let origin_heap = self.origin_heap;
        #[cfg(target_os = "linux")]
        let gen = self.unicode_generation;
        #[cfg(target_os = "linux")]
        let eventfd = self.hub.eventfd();
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                let mut eventfd_fired = false;
                crate::io::uring::drain_cqes(
                    ring,
                    &mut self.pending,
                    &mut self.buffer_pool,
                    &mut self.fd_states,
                    &mut self.completions,
                    origin_heap,
                    gen,
                    &mut eventfd_fired,
                );
                if eventfd_fired {
                    if let Some(efd) = eventfd {
                        crate::io::eventfd::drain(efd);
                        // Re-arm of a single SQE into a 256-deep ring is
                        // infallible in practice; assert so a regression is loud
                        // in tests rather than a silent deaf bridge in release.
                        let rearmed = crate::io::uring::arm_eventfd_poll(ring, efd);
                        debug_assert!(
                            rearmed.is_ok(),
                            "eventfd bridge re-arm failed in poll path: {:?}",
                            rearmed
                        );
                    }
                }
            }
            PlatformBackend::ThreadPool => {}
        }
    }

    /// Drain the shared completion hub (thread-pool workers + stdin worker) into
    /// self.completions. This is the single hub drain site: `drain_raw`
    /// decrements `in_flight` once per item, and `cook_raw` turns each
    /// `RawCompletion` into a `Completion` — returning `None` (and so discarding)
    /// a cancelled op whose `pending` entry is already gone.
    fn drain_hub(&mut self) {
        let origin_heap = self.origin_heap;
        let gen = self.unicode_generation;
        let AsyncBackendInner {
            ref mut hub,
            ref mut pending,
            ref mut cancelled,
            ref mut retired,
            ref mut fd_states,
            ref mut buffer_pool,
            ref mut completions,
            ..
        } = *self;
        for rc in hub.drain_raw() {
            let id = SubmissionId::from_raw(match &rc {
                crate::io::threadpool::RawCompletion::Pool(pc) => pc.id,
                crate::io::threadpool::RawCompletion::Stdin(sc) => sc.id,
            });
            let cooked = cook_raw(
                rc,
                pending,
                cancelled,
                fd_states,
                buffer_pool,
                origin_heap,
                gen,
            );
            hub.forget_stop(id);
            if let Some(c) = cooked {
                completions.push_back(c);
            }
        }
        Self::close_drained_fds(pending, fd_states, retired);
    }

    /// Close every retired descriptor no submitted operation names any more.
    ///
    /// A descriptor reaches `retired` when its port was closed while an
    /// operation still held it; this is where it is finally handed back to the
    /// OS. Its `fd_states` entry goes with it, so per-fd buffering never spans
    /// two ports that happened to share a number.
    fn close_drained_fds(
        pending: &HashMap<SubmissionId, PendingOp>,
        fd_states: &mut HashMap<PortKey, FdState>,
        retired: &mut HashMap<RawFd, std::os::unix::io::OwnedFd>,
    ) {
        if retired.is_empty() {
            return;
        }
        retired.retain(|fd, _owned| {
            let still_named = pending
                .values()
                .any(|op| matches!(op, PendingOp::Port { port_key, .. } if port_key.names_fd(*fd)));
            if !still_named {
                crate::io::types::discard_fd_state(fd_states, *fd);
            }
            // Dropping the `OwnedFd` with the entry is what closes it.
            still_named
        });
    }

    /// Submit a stdin operation.
    fn submit_stdin(&mut self, id: SubmissionId, op: &PortOp) -> Result<SubmissionId, String> {
        // The stdin worker reports through the shared hub like every other
        // worker — hand it a sender clone and the bridge eventfd at spawn.
        let sender = self.hub.sender();
        let eventfd = self.hub.eventfd();
        let stdin_thread = self
            .stdin_thread
            .get_or_insert_with(|| StdinThread::new(sender, eventfd));
        // The worker reads; it has no write, socket or seek path.
        let op_kind = match op {
            PortOp::ReadLine { .. } => StdinOpKind::ReadLine,
            PortOp::Read { count, .. } => StdinOpKind::Read { count: *count },
            PortOp::ReadAll => StdinOpKind::ReadAll,
            PortOp::ReadExact { .. }
            | PortOp::Write { .. }
            | PortOp::Flush
            | PortOp::Accept { .. }
            | PortOp::SendTo { .. }
            | PortOp::RecvFrom { .. }
            | PortOp::Shutdown { .. } => {
                return Err("io/submit: unsupported operation on stdin".into())
            }
        };
        stdin_thread.submit(id, op_kind)?;
        // Count the stdin request in the combined in-flight tally so the
        // scheduler knows to block on the hub for its completion.
        self.hub.note_submit();
        let buf_handle = self.buffer_pool.alloc(0);
        self.pending.insert(
            id,
            PendingOp::Port {
                op: op.clone(),
                port_key: PortKey::Stdin,
                port: Value::NIL,
                buffer_handle: Some(buf_handle),
                listener_kind: None,
                filled: 0,
                // The stdin worker owns its own blocking read; nothing here
                // resubmits through the ring, so there is no link to re-arm.
                timeout: None,
            },
        );
        Ok(id)
    }

    /// Handle Seek and Tell as immediate completions.
    ///
    /// Called from AsyncBackend::submit after port_key is determined and before
    /// buffer allocation. Seek/Tell are synchronous (non-blocking lseek calls)
    /// and never go to io_uring or the thread pool.
    ///
    /// # Buffer invariant
    /// After Seek: the per-fd buffer is cleared and status reset to Open.
    /// After Tell: buffer is read-only; the formula is kernel_offset - buffer.len().
    fn handle_seek_tell(
        &mut self,
        id: SubmissionId,
        port: &Port,
        port_key: &PortKey,
        op: &IoOp,
    ) -> Result<SubmissionId, String> {
        if port.kind() != PortKind::File {
            let err_msg = match op {
                IoOp::Seek { .. } => {
                    format!("port/seek: expected file port, got {:?}", port.kind())
                }
                IoOp::Tell => format!("port/tell: expected file port, got {:?}", port.kind()),
                _ => unreachable!(),
            };
            self.completions.push_back(Completion::err(
                id,
                crate::io::io_error("type-error", err_msg, self.origin_heap),
            ));
            return Ok(id);
        }

        let result = match op {
            IoOp::Seek { offset, whence } => {
                // Discard buffered bytes — kernel offset and logical position diverge otherwise.
                if let Some(state) = self.fd_states.get_mut(port_key) {
                    state.buffer.clear();
                }
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, *offset, *whence) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/seek: fd unavailable",
                    ))
                })
            }
            IoOp::Tell => {
                let buffer_len: i64 = self
                    .fd_states
                    .get(port_key)
                    .map(|state| state.buffer.len() as i64)
                    .unwrap_or(0);
                port.with_fd(|fd| {
                    let raw = fd.as_raw_fd();
                    let ret = unsafe { libc::lseek(raw, 0, libc::SEEK_CUR) };
                    if ret < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(Value::int(ret as i64 - buffer_len))
                    }
                })
                .unwrap_or_else(|| {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "port/tell: fd unavailable",
                    ))
                })
            }
            _ => unreachable!(),
        };

        let origin_heap = self.origin_heap;
        self.completions.push_back(Completion::new(
            id,
            result.map_err(|e| crate::io::io_error("io-error", e.to_string(), origin_heap)),
        ));
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
