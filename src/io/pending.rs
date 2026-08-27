//! PendingOp — in-flight async I/O operation tracking.

use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, PortOp};
use crate::io::types::PortKey;
use crate::io::SubmissionId;
use crate::port::PortKind;
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::os::unix::io::RawFd;
use std::time::Duration;

/// What kind of operation a worker ran.
///
/// A completion carries this back beside its id, so the entry the id resolves
/// through can be checked against the operation that actually finished. Without
/// it a completion resolving to the wrong entry is silent, and the arm it
/// matches applies its own ownership rules to another operation's payload:
/// `ProcessWait` reclaims a `Box<siginfo_t>`, `Connect` and `Open` take
/// ownership of a descriptor, the port arms write through a fiber's buffer. Each
/// of those frees or dereferences memory belonging to something else.
///
/// The kinds are coarser than [`PendingOp`] on purpose: they name what a worker
/// can tell you it did. `ev/poll-fd` and the `chan/wait-ready` park run the same
/// operation and are both [`OpKind::Poll`], which is why the check is
/// [`PendingOp::accepts`] rather than an equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpKind {
    /// Stream or socket I/O on a descriptor a port owns.
    Port,
    Connect,
    Sleep,
    ProcessWait,
    Open,
    Task,
    Resolve,
    Watch,
    Signal,
    /// A readiness wait on a bare descriptor.
    Poll,
}

/// Pending async I/O operation.
///
/// Three variants matching the three port lifecycles:
/// - `Port`: operates on an existing port (stream I/O, accept, datagram, shutdown)
/// - `Connect`: creates a new port on completion (no existing port)
/// - `Sleep`: portless timer
pub(crate) enum PendingOp {
    /// Operation on an existing port.
    Port {
        op: PortOp,
        port_key: PortKey,
        port: Value,
        /// BufferPool handle for non-read operations. `None` for Read/ReadLine
        /// (which use pre-allocated fiber-heap buffers instead).
        buffer_handle: Option<BufferHandle>,
        /// For Accept: which kind of listener (TcpListener or UnixListener).
        listener_kind: Option<PortKind>,
        /// Bytes of this operation's payload already transferred: read into
        /// the fiber's pre-allocated buffer, or written out to the fd. Both
        /// directions resubmit the remainder from this offset, and the
        /// completion reports `filled + result_code`. Zero for ops that move
        /// no payload.
        filled: usize,
        /// The request's timeout, carried so a resubmission can re-arm the
        /// `LinkTimeout` that bounds it. A payload too large for one syscall
        /// completes over several SQEs, and `:timeout` means "give up after
        /// this long" for each of them rather than for the first alone.
        /// `None` leaves the operation unbounded.
        ///
        /// Only the io_uring backend re-arms a `LinkTimeout`; the thread pool
        /// bounds the op in the worker, so on that platform every submit site
        /// still fills this field and nothing reads it back.
        timeout: Option<Duration>,
    },
    /// Connect to a remote address.
    Connect {
        #[allow(dead_code)]
        addr: ConnectAddr,
        buffer_handle: BufferHandle,
        /// io_uring: pre-created socket fd. Thread pool: set to result fd
        /// on completion. Cleared on connect failure (fd closed).
        connect_fd: Option<RawFd>,
        /// Pre-allocated port Value (born in the solver's region at the call site).
        port: Value,
    },
    /// Async timer. No port.
    Sleep { buffer_handle: BufferHandle },
    /// Waiting for subprocess exit via IORING_OP_WAITID.
    ///
    /// SAFETY: `siginfo` is a heap-allocated `siginfo_t` (via Box::into_raw).
    /// It must live until the CQE arrives. Released in completion processing.
    ProcessWait {
        buffer_handle: BufferHandle,
        handle_val: Value, // ProcessHandle — to cache exit code on completion
        siginfo: *mut libc::siginfo_t, // kernel fills this when child exits
    },
    /// Open a file path. Creates a new port on completion.
    ///
    /// For io_uring: the null-terminated path bytes are stored in the buffer
    /// pool slot (via buffer_handle) so they stay pinned until the CQE arrives.
    /// For thread pool: path is owned by the PoolOp::Open; buffer_handle is a
    /// dummy allocation (0 bytes).
    Open {
        /// The file path (for error messages).
        path: String,
        buffer_handle: BufferHandle,
        /// Pre-allocated port Value (born in the solver's region at the call site).
        port: Value,
    },
    /// Background task — arbitrary closure running on thread pool.
    Task { buffer_handle: BufferHandle },
    /// DNS resolution via getaddrinfo(3). Portless.
    Resolve { buffer_handle: BufferHandle },
    /// Waiting for filesystem watch events (inotify/kqueue).
    WatchNext {
        watcher: Value,
        buffer_handle: BufferHandle,
    },
    /// Waiting for POSIX signal deliveries (signalfd/kqueue).
    SigNext {
        receiver: Value,
        buffer_handle: BufferHandle,
    },
    /// Poll a raw fd for readiness. Portless.
    PollFd { buffer_handle: BufferHandle },
    /// Park a `chan/wait-ready` selector on its wake fd.  Portless.
    /// The guard owns the wake fd(s) and the wake-list registrations;
    /// dropping this PendingOp (completion, cancellation, or backend
    /// teardown) closes the fd(s) and deregisters automatically.
    ChanSelectPark {
        buffer_handle: BufferHandle,
        #[allow(dead_code)] // kept alive for its Drop side effect
        guard: crate::primitives::chan::ChanSelectGuard,
    },
}

impl PendingOp {
    /// Get the BufferHandle, if any. Returns `None` for read operations
    /// (which use pre-allocated fiber-heap buffers) and `Some(handle)` for
    /// all other operations.
    pub(crate) fn buffer_handle(&self) -> Option<BufferHandle> {
        match self {
            PendingOp::Port { buffer_handle, .. } => *buffer_handle,
            PendingOp::Connect { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Sleep { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ProcessWait { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Open { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Task { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Resolve { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::WatchNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::SigNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::PollFd { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ChanSelectPark { buffer_handle, .. } => Some(*buffer_handle),
        }
    }

    /// The heap values this operation holds and a completion dereferences when
    /// it cooks a result: the port it names, the caller's pre-allocated buffer
    /// or result struct, the payload a write hands over, the process handle it
    /// caches an exit code in, the watcher or receiver it reads through.
    ///
    /// None of them are the operation's own allocations. Each was born in a
    /// region of the fiber that asked, so each is live only while that fiber's
    /// regions are — which is what [`OperandSite`] records and
    /// [`PendingTable::take`] checks.
    ///
    /// Both matches are exhaustive on purpose: a variant that gains a value
    /// field and does not name it here loses that protection silently. A slot
    /// an operation does not use reads `Value::NIL`, which carries no region
    /// and so answers every question about one with "nothing to lose".
    pub(crate) fn operands(&self) -> [Value; MAX_OPERANDS] {
        let mut out = [Value::NIL; MAX_OPERANDS];
        match self {
            PendingOp::Port { op, port, .. } => {
                out[0] = *port;
                out[1] = match op {
                    PortOp::ReadLine { buffer }
                    | PortOp::Read { buffer, .. }
                    | PortOp::ReadExact { buffer, .. } => *buffer,
                    PortOp::Write { data } | PortOp::SendTo { data, .. } => *data,
                    PortOp::Accept { accept_port, .. } => *accept_port,
                    PortOp::RecvFrom { result, .. } => *result,
                    PortOp::ReadAll | PortOp::Flush | PortOp::Shutdown { .. } => Value::NIL,
                };
            }
            PendingOp::Connect { port, .. } | PendingOp::Open { port, .. } => out[0] = *port,
            PendingOp::ProcessWait { handle_val, .. } => out[0] = *handle_val,
            PendingOp::WatchNext { watcher, .. } => out[0] = *watcher,
            PendingOp::SigNext { receiver, .. } => out[0] = *receiver,
            PendingOp::Sleep { .. }
            | PendingOp::Task { .. }
            | PendingOp::Resolve { .. }
            | PendingOp::PollFd { .. }
            | PendingOp::ChanSelectPark { .. } => {}
        }
        out
    }

    /// Could an operation of kind `kind` have filed this entry?
    ///
    /// A completion answers "no" only when the submission table and the
    /// completion disagree about what is in flight under one id. Nothing
    /// downstream can be trusted then, so the caller reports the disagreement
    /// rather than cooking a result through an arm the payload does not fit.
    pub(crate) fn accepts(&self, kind: OpKind) -> bool {
        matches!(
            (self, kind),
            (PendingOp::Port { .. }, OpKind::Port)
                | (PendingOp::Connect { .. }, OpKind::Connect)
                | (PendingOp::Sleep { .. }, OpKind::Sleep)
                | (PendingOp::ProcessWait { .. }, OpKind::ProcessWait)
                | (PendingOp::Open { .. }, OpKind::Open)
                | (PendingOp::Task { .. }, OpKind::Task)
                | (PendingOp::Resolve { .. }, OpKind::Resolve)
                | (PendingOp::WatchNext { .. }, OpKind::Watch)
                | (PendingOp::SigNext { .. }, OpKind::Signal)
                | (PendingOp::PollFd { .. }, OpKind::Poll)
                | (PendingOp::ChanSelectPark { .. }, OpKind::Poll)
        )
    }

    /// The name of this entry's operation, for a report about an id whose entry
    /// and completion disagree.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            PendingOp::Port { .. } => "port I/O",
            PendingOp::Connect { .. } => "connect",
            PendingOp::Sleep { .. } => "sleep",
            PendingOp::ProcessWait { .. } => "process wait",
            PendingOp::Open { .. } => "open",
            PendingOp::Task { .. } => "task",
            PendingOp::Resolve { .. } => "resolve",
            PendingOp::WatchNext { .. } => "watch",
            PendingOp::SigNext { .. } => "signal",
            PendingOp::PollFd { .. } => "poll",
            PendingOp::ChanSelectPark { .. } => "channel park",
        }
    }

    pub(super) fn filled(&self) -> usize {
        match self {
            PendingOp::Port { filled, .. } => *filled,
            _ => 0,
        }
    }

    /// The request's timeout, for a backend re-arming the bound on a
    /// resubmission. `None` for ops that carry no deadline.
    ///
    /// Only `io::uring::drain` calls this, so the allow is narrowed to the
    /// platforms that compile that module out rather than blanket `dead_code`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn timeout(&self) -> Option<Duration> {
        match self {
            PendingOp::Port { timeout, .. } => *timeout,
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(super) fn set_filled(&mut self, val: usize) {
        if let PendingOp::Port { filled, .. } = self {
            *filled = val;
        }
    }

    /// Give back everything this operation owns, without building a value for
    /// it: the pooled buffer, a descriptor the completion would have wrapped in
    /// a port, and the `siginfo_t` a process wait allocated.
    ///
    /// `result_fd` is the raw completion's result code, which for a connect, an
    /// open or an accept is the descriptor the operation obtained. Nobody will
    /// take it now, so it is closed here rather than leaked.
    pub(crate) fn retire(self, result_fd: i32, buffer_pool: &mut BufferPool) {
        if let Some(bh) = self.buffer_handle() {
            buffer_pool.release(bh);
        }
        match self {
            PendingOp::Connect { connect_fd, .. } => {
                // io_uring pre-creates the socket; the pool reports it here.
                if let Some(fd) = connect_fd.or(if result_fd > 0 { Some(result_fd) } else { None })
                {
                    // SAFETY: nothing else holds this descriptor — the port that
                    // would have owned it is never built.
                    unsafe { libc::close(fd) };
                }
            }
            PendingOp::Open { .. } if result_fd > 0 => {
                // SAFETY: as above — the port for this descriptor is never built.
                unsafe { libc::close(result_fd) };
            }
            // An accept that succeeded owns a descriptor too: the connection the
            // kernel handed back. `listener_kind` is what says this entry is an
            // accept, and a negative `result_fd` is a failure or a cancellation,
            // which produced none. A server whose accept loop is aborted retires
            // an accept on every round, so leaving this one open leaks a socket
            // per round.
            PendingOp::Port {
                listener_kind: Some(_),
                ..
            } if result_fd > 0 => {
                // SAFETY: as above — the port for this descriptor is never built.
                unsafe { libc::close(result_fd) };
            }
            PendingOp::ProcessWait { siginfo, .. } if !siginfo.is_null() => {
                // SAFETY: allocated by `Box::into_raw` at submit; reclaimed once.
                drop(unsafe { Box::from_raw(siginfo) });
            }
            _ => {}
        }
    }
}

/// The most heap values one operation holds: the port it names and the one
/// buffer, payload or result struct the caller reserved for it.
/// [`PendingOp::operands`] fills the rest with `Value::NIL`.
pub(crate) const MAX_OPERANDS: usize = 2;

/// Where one operand lived when its entry was filed: the region, and the
/// incarnation of that region current at the time.
///
/// The generation is what makes "is it still there?" an exact question. Region
/// ids are recycled, so the id alone cannot tell this incarnation from the next,
/// and the value's own page header cannot either — a freed page that has been
/// re-claimed carries its new owner's stamp. The store's counter moves only on a
/// free (docs/impl/region/generations.md § "Region generations").
#[derive(Clone, Copy)]
struct OperandSite {
    region: u32,
    generation: u32,
}

impl OperandSite {
    /// Where each of `op`'s operands lives, on `heap`. `None` for a slot the
    /// operation does not use, and for a value `heap` does not own: a worker
    /// reading a parent-heap value is the tolerated cross-store borrow, and this
    /// store's generation counter says nothing about another store's frees.
    fn of(
        op: &PendingOp,
        heap: &crate::value::fiberheap::FiberHeap,
    ) -> [Option<OperandSite>; MAX_OPERANDS] {
        op.operands().map(|v| {
            if !heap.value_in_region_store(v) {
                return None;
            }
            crate::value::arena::region_of(heap, v).map(|r| OperandSite {
                region: r.get(),
                generation: heap.region_generation(r.get()),
            })
        })
    }

    /// Whether the region this operand was born in has since been freed.
    fn gone(&self, heap: &crate::value::fiberheap::FiberHeap) -> bool {
        heap.region_generation(self.region) != self.generation
    }
}

/// One in-flight operation, and where the values it holds lived when it was
/// filed. The two travel together because a completion reads both: the entry to
/// build a result from, and the sites to decide whether it may.
struct Entry {
    op: PendingOp,
    /// The heap the sites below were read from, so a completion can tell that
    /// it is asking the store that answered before. Generations are per store —
    /// one store's counter for an id says nothing about another's.
    heap: *mut crate::value::fiberheap::FiberHeap,
    sites: [Option<OperandSite>; MAX_OPERANDS],
}

impl Entry {
    /// Whether any region this entry's operands were born in has since been
    /// freed.
    ///
    /// False when the backend recorded no heap — the answer a submission made
    /// outside a region store wants — and false when `heap` is not the store the
    /// sites came from, because the two stores' generation counters are
    /// unrelated numbers.
    fn operands_gone(&self, heap: *mut crate::value::fiberheap::FiberHeap) -> bool {
        if heap.is_null() || heap != self.heap {
            return false;
        }
        // SAFETY: the backend recorded this pointer at submit, and a completion
        // is resolved on the instance that owns it.
        let heap = unsafe { &*heap };
        self.sites.iter().flatten().any(|s| s.gone(heap))
    }
}

/// What a completion found when it looked its submission up.
pub(crate) enum Taken {
    /// The operation, with a fiber waiting for its result. Cook it.
    Live(PendingOp),
    /// The operation, with nobody to receive it. Retire it instead: building
    /// the result would read heap values whose regions the finished fiber's
    /// release may already have freed.
    Cancelled(PendingOp),
    /// The operation, with its operands' regions gone. Retire it as a cancelled
    /// one is — but answer, rather than fall silent.
    ///
    /// The difference from `Cancelled` is who still holds the id. Every
    /// `io/cancel` caller in the scheduler drops its own record of the
    /// submission first, so a cancelled id has no reader and no bookkeeping
    /// left; silence is what it wants. Nobody dropped this one — the fiber that
    /// asked ended without telling anyone — so the scheduler still pairs the id
    /// with that fiber, and only a completion under this id retires the pairing.
    /// Withholding it too would leave the pairing in place and the event loop
    /// waiting on an operation that already finished.
    ///
    /// The answer is an error built from nothing the entry held.
    Stale(PendingOp),
    /// No entry under this id — already reaped, or never filed.
    Unknown,
}

/// The operations a backend has in flight, and which of them no fiber will
/// receive a result for.
///
/// The two facts live together because one decision reads both. An arriving
/// completion asks a single question — "does anybody want this?" — and
/// [`take`](Self::take) answers it, so neither backend can honour the
/// cancellation contract on one half and forget it on the other.
///
/// A cancelled operation KEEPS its entry until its own completion arrives. The
/// worker it runs on and the descriptor it names come back with that
/// completion; dropping the entry at the cancel would strand both.
#[derive(Default)]
pub(crate) struct PendingTable {
    ops: HashMap<SubmissionId, Entry>,
    /// Ids whose result no fiber will receive. Every `io/cancel` caller in the
    /// scheduler drops its own record of the submission before cancelling, so
    /// the id is marked here precisely when there is no longer a reader.
    cancelled: HashSet<SubmissionId>,
    /// Ids [`stale_to_stop`](Self::stale_to_stop) has already reported, so a
    /// backend that sweeps on every drain asks each worker once. Unlike
    /// `cancelled` this changes nothing about the answer: the completion these
    /// ids are waiting for is still delivered.
    stop_asked: HashSet<SubmissionId>,
}

impl PendingTable {
    pub(crate) fn new() -> Self {
        PendingTable::default()
    }

    /// File a fresh submission's entry under the id it was dispatched with,
    /// recording where its operands live.
    pub(crate) fn insert(
        &mut self,
        id: SubmissionId,
        op: PendingOp,
        heap: *mut crate::value::fiberheap::FiberHeap,
    ) {
        let sites = if heap.is_null() {
            [None; MAX_OPERANDS]
        } else {
            // SAFETY: the caller submits on the instance whose heap this is, and
            // the operands were allocated on it moments ago.
            OperandSite::of(&op, unsafe { &*heap })
        };
        self.ops.insert(id, Entry { op, heap, sites });
    }

    /// Take the entry a completion resolves through, and say whether anybody
    /// is waiting for its result.
    ///
    /// Two ways to have no reader, and they are answered differently because a
    /// different amount of bookkeeping is left. The id was cancelled — a caller
    /// dropped its record of the submission and said so, so there is nothing
    /// left to tell. Or an operand's region is gone, which says the fiber that
    /// asked ended without anybody cancelling for it, and the scheduler is still
    /// holding the pairing (see [`Taken::Stale`]).
    pub(crate) fn take(
        &mut self,
        id: SubmissionId,
        heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Taken {
        let was_cancelled = self.cancelled.remove(&id);
        self.stop_asked.remove(&id);
        match self.ops.remove(&id) {
            Some(e) if was_cancelled => Taken::Cancelled(e.op),
            Some(e) if e.operands_gone(heap) => Taken::Stale(e.op),
            Some(e) => Taken::Live(e.op),
            None => Taken::Unknown,
        }
    }

    /// The in-flight ids whose operands' regions are gone and whose operation
    /// nobody has asked to stop yet.
    ///
    /// An operation that parks completes when something outside this process
    /// acts, and the fiber that would have read the result is what went away,
    /// so the backend ends these itself (src/io/AGENTS.md § "Ending an
    /// operation whose operands are gone"). Reporting an id records it here, so
    /// a caller sweeping on every drain asks each worker once; the record
    /// leaves with the entry in [`take`](Self::take).
    pub(crate) fn stale_to_stop(
        &mut self,
        heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Vec<SubmissionId> {
        if heap.is_null() {
            return Vec::new();
        }
        let PendingTable {
            ops, stop_asked, ..
        } = self;
        let ids: Vec<SubmissionId> = ops
            .iter()
            .filter(|(id, e)| !stop_asked.contains(*id) && e.operands_gone(heap))
            .map(|(id, _)| *id)
            .collect();
        stop_asked.extend(ids.iter().copied());
        ids
    }

    /// What a completion for an operation whose operands are gone says.
    ///
    /// Built from nothing the entry held — that is the point — so it names the
    /// reason rather than the operation. The only reader is the scheduler,
    /// which retires the pairing this id belongs to and drops the error: the
    /// fiber that would have received it is what went away.
    pub(crate) fn stale_operand_error(
        id: SubmissionId,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> crate::io::Completion {
        crate::io::Completion::err(
            id,
            crate::io::io_error(
                "io-error",
                format!(
                    "io completion {id}: the fiber that requested this operation \
                     ended before it finished, and the values the operation named \
                     went with it"
                ),
                origin_heap,
            ),
        )
    }

    /// Mark `id` as having no reader, so its completion is retired rather than
    /// cooked. A no-op for an id that is not in flight — that operation has
    /// already been reaped and its result already handed to the fiber that
    /// asked, so there is nothing left to withhold and a mark would sit in the
    /// set with no completion coming to clear it.
    pub(crate) fn mark_cancelled(&mut self, id: SubmissionId) {
        if self.ops.contains_key(&id) {
            self.cancelled.insert(id);
        }
    }

    /// Mark every operation still in flight as having no reader. Backend
    /// teardown: the fibers are gone and the heap that carried their values may
    /// be too, so the drain that follows must retire rather than cook.
    ///
    /// Only `quiesce_pending` calls this, and only the ring has a teardown
    /// drain, so the allow is narrowed to the platforms that compile that path
    /// out rather than a blanket `dead_code`. The three below are the same
    /// story: `restore` is the ring's resubmission, `len` and `ids` are what the
    /// teardown loop reads.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn cancel_all(&mut self) {
        self.cancelled.extend(self.ops.keys().copied());
    }

    /// Put a resubmitted operation's entry back. The operation is the same one
    /// — a read that needs another syscall to reach its newline, its count, or
    /// its EOF — so this is one operation's entry moving, not a new submission.
    ///
    /// Its operand sites are recorded again: the `take` that pulled the entry
    /// out consumed the old ones, and the operands are known live right now
    /// because that same `take` answered `Live`. No Elle runs between the two,
    /// so nothing can have been freed in between.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn restore(
        &mut self,
        id: SubmissionId,
        op: PendingOp,
        heap: *mut crate::value::fiberheap::FiberHeap,
    ) {
        self.insert(id, op, heap);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    /// The ids in flight. Callers that submit while iterating take this rather
    /// than borrowing the table.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn ids(&self) -> Vec<SubmissionId> {
        self.ops.keys().copied().collect()
    }

    /// The entry filed under `id`, for a test reporting on an operation still
    /// in flight. Production code takes its entry with [`take`](Self::take),
    /// which is where the cancellation question is answered; a borrow that
    /// skips that question is exactly what this table exists to prevent.
    #[cfg(test)]
    pub(crate) fn get(&self, id: SubmissionId) -> Option<&PendingOp> {
        self.ops.get(&id).map(|e| &e.op)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SubmissionId, &PendingOp)> {
        self.ops.iter().map(|(id, e)| (id, &e.op))
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &PendingOp> {
        self.ops.values().map(|e| &e.op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::pool::BufferPool;

    /// A `Sleep` entry: the one variant that owns nothing but its buffer, so a
    /// table test can file and retire it without a port, a child or a socket.
    fn sleep_op(pool: &mut BufferPool) -> PendingOp {
        PendingOp::Sleep {
            buffer_handle: pool.alloc(0),
        }
    }

    fn id(n: u64) -> SubmissionId {
        SubmissionId::from_raw(n)
    }

    /// What a table test files against when its entries hold no values: a
    /// `Sleep` owns nothing that could be freed, so there is no store to ask.
    fn no_heap() -> *mut crate::value::fiberheap::FiberHeap {
        std::ptr::null_mut()
    }

    /// An entry holding one heap value. `WatchNext` is the shape with a single
    /// operand and nothing else to stand up.
    fn watch_op(pool: &mut BufferPool, watcher: Value) -> PendingOp {
        PendingOp::WatchNext {
            watcher,
            buffer_handle: pool.alloc(0),
        }
    }

    /// An uncancelled submission's completion is handed its entry to cook.
    #[test]
    fn a_live_submission_is_taken_live() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), no_heap());
        assert!(matches!(table.take(id(1), no_heap()), Taken::Live(_)));
        assert!(table.is_empty(), "taking an entry removes it");
    }

    /// A cancelled submission's completion is told so, and only once: the mark
    /// leaves with the entry, so nothing about this id survives to affect a
    /// later lookup.
    #[test]
    fn a_cancelled_submission_is_taken_cancelled_exactly_once() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), no_heap());
        table.mark_cancelled(id(1));
        match table.take(id(1), no_heap()) {
            Taken::Cancelled(op) => op.retire(0, &mut pool),
            _ => panic!("a marked submission must be reported cancelled"),
        }
        assert!(matches!(table.take(id(1), no_heap()), Taken::Unknown));
    }

    /// Cancelling an id that is no longer in flight marks nothing.
    ///
    /// The trap: `io/cancel` races the completion it is trying to prevent, so a
    /// cancel routinely arrives for an operation already reaped and delivered.
    /// A mark filed then would have no completion coming to clear it.
    #[test]
    fn cancelling_a_reaped_submission_marks_nothing() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), no_heap());
        assert!(matches!(table.take(id(1), no_heap()), Taken::Live(_)));

        table.mark_cancelled(id(1));
        assert!(matches!(table.take(id(1), no_heap()), Taken::Unknown));
    }

    /// A resubmitted operation is still in flight, so a cancel still reaches
    /// it.
    ///
    /// A read that needs another syscall to reach its newline, its count or its
    /// EOF leaves the table and comes back (`drain_cqes`). What must not follow
    /// is the entry reading as reaped in between — a cancel issued after the
    /// restore would then mark nothing, and the next completion would be cooked
    /// for a fiber that is gone.
    #[test]
    fn a_resubmitted_operation_can_still_be_cancelled() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), no_heap());
        let op = match table.take(id(1), no_heap()) {
            Taken::Live(op) => op,
            _ => panic!("an unmarked entry is live"),
        };
        table.restore(id(1), op, no_heap());

        table.mark_cancelled(id(1));
        match table.take(id(1), no_heap()) {
            Taken::Cancelled(op) => op.retire(0, &mut pool),
            _ => panic!("a resubmitted operation must still be cancellable"),
        }
    }

    /// An entry whose operand's region is gone has no reader, whether or not
    /// anybody cancelled it.
    ///
    /// The trap: nothing marks this id. The fiber that asked ran to a terminal
    /// state by a path that told no one, so the only evidence left is that the
    /// region its operand lived in has been freed.
    #[test]
    fn an_entry_whose_operand_region_is_gone_is_taken_stale() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let region = h.new_runtime_region();
        let watcher = h.alloc_in_region(
            crate::value::heap::HeapObject::LBox {
                cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
                traits: Value::NIL,
            },
            region,
        );

        table.insert(id(1), watch_op(&mut pool, watcher), heap);
        h.decref_region(region);

        match table.take(id(1), heap) {
            Taken::Stale(op) => op.retire(0, &mut pool),
            _ => panic!("an operation whose operand region is gone has no reader"),
        }
    }

    /// The same entry read while its operand is still there is live.
    ///
    /// The counter-factual for the test above: without it, a check that simply
    /// answered "gone" would pass that one and withhold every completion in the
    /// process.
    #[test]
    fn an_entry_whose_operand_region_lives_is_taken_live() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let region = h.new_runtime_region();
        let watcher = h.alloc_in_region(
            crate::value::heap::HeapObject::LBox {
                cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
                traits: Value::NIL,
            },
            region,
        );

        table.insert(id(1), watch_op(&mut pool, watcher), heap);
        match table.take(id(1), heap) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("an operation whose operands are all there has a reader"),
        }
    }

    /// A resubmission keeps the question askable.
    ///
    /// The trap: `take` consumes the entry's operand sites, and a read that
    /// needs another syscall goes back through `restore`. An entry that came
    /// back without its sites would answer "live" forever after, which is
    /// exactly the answer that reads a freed port.
    #[test]
    fn a_restored_entry_still_checks_its_operands() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let region = h.new_runtime_region();
        let watcher = h.alloc_in_region(
            crate::value::heap::HeapObject::LBox {
                cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
                traits: Value::NIL,
            },
            region,
        );

        table.insert(id(1), watch_op(&mut pool, watcher), heap);
        let op = match table.take(id(1), heap) {
            Taken::Live(op) => op,
            _ => panic!("a live operand reads as live"),
        };
        table.restore(id(1), op, heap);

        h.decref_region(region);
        match table.take(id(1), heap) {
            Taken::Stale(op) => op.retire(0, &mut pool),
            _ => panic!("a resubmitted operation must still lose its reader"),
        }
    }

    /// A table holding one entry whose operand's region has been freed, and the
    /// heap that freed it. `region` is gone; nothing else in the table is.
    fn table_with_a_freed_operand() -> (
        BufferPool,
        PendingTable,
        *mut crate::value::fiberheap::FiberHeap,
    ) {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let region = h.new_runtime_region();
        let watcher = h.alloc_in_region(
            crate::value::heap::HeapObject::LBox {
                cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
                traits: Value::NIL,
            },
            region,
        );

        table.insert(id(1), watch_op(&mut pool, watcher), heap);
        // A second entry that owns nothing, so a sweep reporting everything is
        // distinguishable from one reporting what it should.
        table.insert(id(2), sleep_op(&mut pool), heap);
        h.decref_region(region);
        (pool, table, heap)
    }

    /// An operation whose operands are gone is reported for a stop exactly
    /// once, and only that operation is.
    ///
    /// The trap: a backend sweeps on every drain, which is every tick of the
    /// event loop. Without the record, each tick would ask the same worker
    /// again for an operation whose completion is already on its way.
    #[test]
    fn an_operation_whose_operands_are_gone_is_reported_for_a_stop_once() {
        let (_pool, mut table, heap) = table_with_a_freed_operand();

        assert_eq!(
            table.stale_to_stop(heap),
            vec![id(1)],
            "only the entry whose operand region is gone needs stopping",
        );
        assert!(
            table.stale_to_stop(heap).is_empty(),
            "a second sweep must ask nothing: the first ask is still in flight",
        );
    }

    /// Being asked to stop does not change the answer the operation gets.
    ///
    /// The counter-factual is marking the id cancelled instead, which is the
    /// other way to end an operation early: a cancelled id falls silent, and
    /// the scheduler is still holding the pairing this completion has to
    /// retire.
    #[test]
    fn an_operation_asked_to_stop_is_still_taken_stale() {
        let (mut pool, mut table, heap) = table_with_a_freed_operand();
        assert_eq!(table.stale_to_stop(heap), vec![id(1)]);

        match table.take(id(1), heap) {
            Taken::Stale(op) => op.retire(0, &mut pool),
            _ => panic!("an operation asked to stop must still answer"),
        }
    }

    /// Teardown withholds every result at once.
    #[test]
    fn cancel_all_marks_everything_in_flight() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        for n in 1..=3 {
            table.insert(id(n), sleep_op(&mut pool), no_heap());
        }
        table.cancel_all();
        for n in 1..=3 {
            assert!(
                matches!(table.take(id(n), no_heap()), Taken::Cancelled(_)),
                "submission {n} must be withheld at teardown",
            );
        }
    }
}
