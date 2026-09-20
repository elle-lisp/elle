//! audited: 2026-09-20
//! `AsyncBackend`: the state an in-flight operation is tracked through, and the
//! platform that runs it.
//!
//! io_uring on Linux, the thread pool everywhere else.
//!
//! src/io/AGENTS.md
//! docs/io.md

use crate::io::completion;
use crate::io::pending::{OpKind, PendingOp, PendingTable, Taken};
use crate::io::pool::BufferPool;
use crate::io::request::{
    ConnectAddr, IoOp, IoRequest, PortOp, ProcessHandle, SpawnRequest, TaskFn,
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
use std::os::unix::io::AsRawFd;
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
    /// The operations in flight, and which of them no fiber will receive a
    /// result for. See src/io/AGENTS.md § "I/O Cancellation".
    pending: PendingTable,
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
    /// Who the submission currently being dispatched is on behalf of, recorded
    /// at `submit` entry so each `pending.insert` files its entry against it.
    /// Read only between that entry and the insert, so what a completed
    /// submission leaves behind here is never consulted.
    submitter: crate::io::pending::Submitter,
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
    /// On Linux it tries io_uring first and takes the thread pool if the ring
    /// will not open. Every other platform takes the pool outright.
    ///
    /// This takes the process-default Unicode generation and the default worker
    /// keepalive. A backend serving a VM with a generation of its own, or a
    /// program that asked for its own keepalive, is built through
    /// [`Self::new_with_unicode`].
    pub fn new() -> Result<Self, String> {
        Self::new_with_unicode(crate::config::get().unicode_generation(), None)
    }

    /// Create a new async backend serving a VM with the given Unicode
    /// generation.
    ///
    /// `keepalive` is how long an idle pool worker waits for another operation
    /// before it retires — what the submitting program bound `*io-keepalive*`
    /// to. `None` takes `DEFAULT_KEEPALIVE`. It reaches the pool platform only;
    /// io_uring runs its operations in the kernel and keeps no workers.
    pub fn new_with_unicode(
        gen: crate::segment::Generation,
        keepalive: Option<Duration>,
    ) -> Result<Self, String> {
        let mut platform = Self::create_platform_backend();
        let mut hub = match keepalive {
            Some(d) => CompletionHub::with_keepalive(d),
            None => CompletionHub::new(),
        };
        // On the uring platform, wire the eventfd bridge: a hub worker raises
        // the eventfd after publishing, and a standing POLL_ADD on the ring
        // turns that edge into a CQE so the scheduler's single io_uring wait
        // returns. No-op on the pool-only platforms (no ring, no eventfd).
        Self::wire_eventfd_bridge(&mut platform, &mut hub)?;
        Ok(AsyncBackend {
            inner: RefCell::new(AsyncBackendInner {
                unicode_generation: gen,
                fd_states: HashMap::new(),
                pending: PendingTable::new(),
                completions: VecDeque::new(),
                next_id: 1,
                buffer_pool: BufferPool::new(),
                stdin_thread: None,
                platform,
                hub,
                origin_heap: std::ptr::null_mut(),
                submitter: crate::io::pending::Submitter::detached(std::ptr::null_mut()),
            }),
        })
    }

    /// A backend on the thread-pool platform, whatever this host would pick.
    ///
    /// The pool is what every non-Linux build runs, and what a Linux host runs
    /// when io_uring will not open or `--no-uring` is set. Its wait path is not
    /// the ring's, so a property that holds on one is no evidence about the
    /// other. A test that built the host's default backend would reach the ring
    /// on a Linux desktop and the pool on another machine, checking different
    /// code on each without saying so. This constructor makes the platform the
    /// test's choice rather than the machine's.
    ///
    /// No eventfd bridge is wired. The pool platform has no ring to bridge into,
    /// so its hub channel is the sole waitable, which is the shape a non-Linux
    /// build comes up with.
    #[cfg(test)]
    pub(crate) fn new_thread_pool() -> Result<Self, String> {
        Self::new_thread_pool_with_keepalive(None)
    }

    /// A thread-pool backend whose workers retire after `keepalive` without a
    /// job, as `*io-keepalive*` asks of one. `None` takes the default.
    #[cfg(test)]
    pub(crate) fn new_thread_pool_with_keepalive(
        keepalive: Option<Duration>,
    ) -> Result<Self, String> {
        Ok(AsyncBackend {
            inner: RefCell::new(AsyncBackendInner {
                unicode_generation: crate::config::get().unicode_generation(),
                fd_states: HashMap::new(),
                pending: PendingTable::new(),
                completions: VecDeque::new(),
                next_id: 1,
                buffer_pool: BufferPool::new(),
                stdin_thread: None,
                platform: PlatformBackend::ThreadPool,
                hub: match keepalive {
                    Some(d) => CompletionHub::with_keepalive(d),
                    None => CompletionHub::new(),
                },
                origin_heap: std::ptr::null_mut(),
                submitter: crate::io::pending::Submitter::detached(std::ptr::null_mut()),
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

    /// Bring this backend to rest: drain every in-flight io_uring operation so
    /// no kernel-owned buffer outlives it, then let go of every region it still
    /// holds — its filed entries' operands, and what its unreaped completions
    /// built. Idempotent once nothing is pending and nothing is queued. Called
    /// from `Drop` and from `FiberHeap::quiesce_io_backends`; see docs/io.md
    /// "Backend teardown" and docs/impl/io-inflight.md.
    pub(crate) fn quiesce(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.quiesce_pending();
            // Whatever the drain could not finish will never complete, so
            // nothing else will dispose of its entry and let go of the regions
            // it holds. This is the last moment the store is reachable: a heap
            // runs this on every backend it still carries before its region
            // sweep (`FiberHeap::quiesce_io_backends`), and the `Drop` that
            // calls this again from inside that sweep reaches an empty hold.
            inner.pending.release_holds();
            // A completion nobody reaped is in the same position, one step
            // further on: it was assembled, so it holds what it built rather
            // than what it read (docs/impl/io-inflight.md). Discarding here is
            // what makes the release name a live store, and it empties the
            // queue, so the second call reaches nothing.
            Completion::discard_all(inner.completions.drain(..));
        }
    }

    /// True when the platform backend is io_uring (vs the thread-pool
    /// fallback). Tests gate uring-specific assertions on this.
    #[cfg(all(target_os = "linux", test))]
    pub(crate) fn is_uring(&self) -> bool {
        matches!(self.inner.borrow().platform, PlatformBackend::Uring(_))
    }

    /// How long this backend's idle pool workers wait for another operation
    /// before retiring — what `*io-keepalive*` asked for, or the default.
    #[cfg(test)]
    pub(crate) fn keepalive(&self) -> Duration {
        self.inner.borrow().hub.keepalive()
    }

    /// The ids of every operation still in flight, ascending. Tests pin the
    /// submission frame with it: one submission files one pending entry, and
    /// the entry is keyed by the id `submit` returned.
    #[cfg(test)]
    pub(crate) fn pending_ids(&self) -> Vec<SubmissionId> {
        let mut ids = self.inner.borrow().pending.ids();
        ids.sort();
        ids
    }
}

impl Drop for AsyncBackend {
    fn drop(&mut self) {
        // Bring the ring to a quiescent state before its buffer pool and its
        // submission and completion queues are freed. An operation still in
        // flight — for example an `io/submit` no `io/wait` ever reaped — leaves
        // the kernel holding a write pointer into a pooled buffer. Reaping it
        // here keeps that write out of freed heap.
        self.quiesce();
    }
}

mod convert;
mod drain;
mod externals;
mod poll;
mod requests;
mod submit;

impl crate::io::IoBackend for AsyncBackend {
    fn submit(
        &self,
        request: &IoRequest,
        submitter: crate::io::pending::Submitter,
    ) -> Result<SubmissionId, String> {
        self.submit(request, submitter)
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
                // Descriptor 0 is process-wide: it outlives every `Port` that
                // names it, so there is no number here to keep out of the OS's
                // hands.
                descriptor: None,
                buffer_handle: Some(buf_handle),
                listener_kind: None,
                filled: 0,
                // The stdin worker owns its own blocking read; nothing here
                // resubmits through the ring, so there is no link to re-arm.
                timeout: None,
            },
            self.submitter,
        );
        Ok(id)
    }

    /// Answer `Seek` and `Tell` inside the submit call.
    ///
    /// `AsyncBackend::submit` calls this once it has the `PortKey` and before it
    /// reserves a buffer. Both operations are one `lseek(2)`, which does not
    /// block, so neither reaches io_uring or the thread pool.
    ///
    /// A seek clears the per-fd buffer, because the kernel offset and the
    /// logical position diverge otherwise. A tell leaves the buffer alone and
    /// answers the kernel offset less the bytes still buffered.
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
            let birth = crate::io::Birthplace::on(self.origin_heap);
            self.completions
                .push_back(Completion::failed(id, birth, "type-error", err_msg));
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

        let mut birth = crate::io::Birthplace::on(self.origin_heap);
        let result = result.map_err(|e| birth.error("io-error", e.to_string()));
        self.completions
            .push_back(Completion::new(id, birth, result));
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
