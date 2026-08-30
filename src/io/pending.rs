//! PendingOp — in-flight async I/O operation tracking.

use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, PortOp};
use crate::io::types::PortKey;
use crate::io::SubmissionId;
use crate::port::PortKind;
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::os::unix::io::{OwnedFd, RawFd};
use std::rc::Rc;
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
        /// This operation's share of the descriptor it names.
        ///
        /// A worker resolves the number when it runs rather than when the
        /// operation was submitted, so the number must not go back to the OS
        /// while this entry exists: a new socket handed it would be the one the
        /// worker reads. The share is what holds it — the number is given back
        /// with the last share, which is this one whenever the port has gone
        /// first (src/io/AGENTS.md § "Descriptor retirement").
        ///
        /// `None` for an operation on a port that owns no descriptor: the
        /// stdio numbers are process-wide and outlive every `Port` that names
        /// them.
        #[allow(dead_code)] // kept alive for its Drop side effect
        descriptor: Option<Rc<OwnedFd>>,
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
    /// region of the fiber that asked, and nothing else counts a reference held
    /// by the pending table — so [`OperandHold`] retains exactly this list.
    ///
    /// Both matches are exhaustive on purpose: a variant that gains a value
    /// field and does not name it here goes unretained, and its region can then
    /// be freed under the completion that reads it. A slot an operation does not
    /// use reads `Value::NIL`, which carries no region and so retains nothing.
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

/// Every heap value one entry holds: its operands, plus the fiber that asked.
/// [`OperandHold`] retains all of them, because the entry reads all of them.
const HELD_VALUES: usize = MAX_OPERANDS + 1;

/// Who a submission is on behalf of: the heap its operands live on, and the
/// fiber that will read its result.
///
/// The two travel together because one submission answers for both — the heap
/// is what a retain and a release reach, and the fiber is what says whether a
/// result has anywhere to go.
#[derive(Clone, Copy)]
pub(crate) struct Submitter {
    heap: *mut crate::value::fiberheap::FiberHeap,
    fiber: Value,
}

impl Submitter {
    /// A submission a fiber of this scheduler is waiting on.
    pub(crate) fn new(heap: *mut crate::value::fiberheap::FiberHeap, fiber: Value) -> Submitter {
        Submitter { heap, fiber }
    }

    /// A submission no fiber of this scheduler is waiting on. Three reach this:
    /// one forwarded for a child scheduler, whose reader is a queue and a wake
    /// box rather than a fiber here; the WASM host's top-level path, which
    /// submits and waits inline and is its own reader; and one a test issues
    /// directly. Nothing withholds such a result and nothing sweeps it —
    /// `io/cancel` is how a reader that is not a fiber lets go.
    pub(crate) fn detached(heap: *mut crate::value::fiberheap::FiberHeap) -> Submitter {
        Submitter {
            heap,
            fiber: Value::NIL,
        }
    }

    /// A submission a test issues directly: on the leaked test heap, and on
    /// behalf of no fiber. A test that is about the fiber builds its own with
    /// [`new`](Self::new).
    #[cfg(test)]
    pub(crate) fn for_test() -> Submitter {
        Submitter::detached(crate::value::arena::leaked_test_heap())
    }

    /// The heap this submission's operands live on and its results are born on.
    pub(crate) fn heap(&self) -> *mut crate::value::fiberheap::FiberHeap {
        self.heap
    }

    /// Whether the fiber that asked has reached a terminal state, so no result
    /// can reach it. False for a detached submission — there is no fiber to
    /// have ended.
    ///
    /// `try_with` rather than `with`: a fiber currently executing on the VM has
    /// been taken out of its handle, and a borrow of one panics. Such a fiber is
    /// running, which is the opposite of terminal, so the unavailable answer and
    /// the false one are the same answer.
    fn asker_finished(&self) -> bool {
        use crate::value::fiber::FiberStatus;
        let Some(handle) = self.fiber.as_fiber() else {
            return false;
        };
        handle
            .try_with(|f| matches!(f.status, FiberStatus::Dead | FiberStatus::Error))
            .unwrap_or(false)
    }
}

/// The reference a submitted operation holds on the regions that keep its
/// operands allocated, for as long as it is in flight.
///
/// The pending table is runtime-side state that no free-time cascade reaches, so
/// nothing counts a reference held by it unless the seam counts its own — the
/// position `chan/send`'s message is in (docs/impl/region/effects.md § `Sends`).
/// [`release`](Self::release) is idempotent, and `Drop` runs it, so an entry
/// disposed of by any route lets go exactly once.
struct OperandHold {
    /// The store the regions below belong to. Null once released, which is what
    /// makes a second release a no-op.
    heap: *mut crate::value::fiberheap::FiberHeap,
    /// The reclamation root of each held value's region — not the region the
    /// value sits in, which for an adopted operand has no count to hold. See
    /// [`take`](Self::take).
    regions: [Option<crate::hir::region::RuntimeRegion>; HELD_VALUES],
}

impl OperandHold {
    /// Retain what keeps each of the entry's held values allocated: `op`'s
    /// operands, and the fiber that asked.
    ///
    /// The fiber is held for the same reason the operands are, and it is read
    /// more often than any of them — `asker_finished` dereferences it on every
    /// drain. A fiber whose region went while its operation was still in flight
    /// would be read there, which is a use-after-free on the very check that
    /// exists to notice the fiber is gone.
    ///
    /// A value the store does not own retains nothing: a worker reading a
    /// parent-heap value is the tolerated cross-store borrow, and this store
    /// neither counts nor may free another's regions. An immediate has no region
    /// and retains nothing either.
    fn take(op: &PendingOp, submitter: Submitter) -> OperandHold {
        let heap = submitter.heap;
        if heap.is_null() {
            return OperandHold::released();
        }
        // SAFETY: the caller submits on the instance whose heap this is, and the
        // values were allocated on it moments ago.
        let h = unsafe { &mut *heap };
        let mut held = [Value::NIL; HELD_VALUES];
        held[..MAX_OPERANDS].copy_from_slice(&op.operands());
        held[MAX_OPERANDS] = submitter.fiber;
        let regions = held.map(|v| {
            if !h.value_in_region_store(v) {
                return None;
            }
            // What is retained is the region reclamation listens to, not the one
            // the value sits in: an operand adopted into an activation's subtree
            // has no count of its own for a retain to raise (src/io/AGENTS.md §
            // "A hold retains what reclamation listens to"). The root is what
            // `release` gives back, so the two are the same region by
            // construction.
            let root = crate::value::arena::region_of(h, v).map(|r| h.reclaim_root(r));
            debug_assert!(
                root.is_none_or(|r| !h.region_is_owned(r)),
                "a reclamation root is Counted by definition — an Owned one means \
                 the owner walk stopped short of the region that reclaims",
            );
            crate::value::arena::incref_for_escape(
                h,
                root,
                crate::value::arena::EscapeSite::IoSubmit,
            );
            root
        });
        OperandHold { heap, regions }
    }

    /// A hold on nothing.
    fn released() -> OperandHold {
        OperandHold {
            heap: std::ptr::null_mut(),
            regions: [None; HELD_VALUES],
        }
    }

    /// Let go of every region this hold retained. Idempotent: a released hold
    /// names no store and so reaches nothing.
    fn release(&mut self) {
        if self.heap.is_null() {
            return;
        }
        // SAFETY: the store this hold named at the retain. Every route that
        // disposes of an entry runs while that store is live — a completion is
        // resolved on it, and the teardown release runs before the store tears
        // its regions down (`FiberHeap::quiesce_io_backends`).
        let h = unsafe { &mut *self.heap };
        for region in self.regions.iter_mut() {
            crate::value::arena::decref_region(h, region.take());
        }
        self.heap = std::ptr::null_mut();
    }
}

impl Drop for OperandHold {
    fn drop(&mut self) {
        self.release();
    }
}

/// One in-flight operation: what its completion is built from, who asked for it,
/// and the hold that keeps the values it reads.
struct Entry {
    op: PendingOp,
    submitter: Submitter,
    hold: OperandHold,
}

/// An operation taken out of the table, still holding its operands and still
/// naming who asked for it.
///
/// The hold travels with the operation rather than being let go at the take,
/// because the take is what a completion reads the operands *through*: once the
/// fiber that asked has gone, the entry's hold is the last reference to its port
/// and its buffer, and releasing first would free them under the assembly. The
/// hold goes when this does — after the result is built — or moves back into the
/// table with [`PendingTable::restore`], which is also why the submitter rides
/// along: a resubmission is the same operation, so it answers to the same fiber.
pub(crate) struct TakenOp {
    op: PendingOp,
    submitter: Submitter,
    hold: OperandHold,
}

impl TakenOp {
    /// Give back everything this operation owns without building a value for
    /// it, then let go of its operands. See [`PendingOp::retire`].
    pub(crate) fn retire(self, result_fd: i32, buffer_pool: &mut BufferPool) {
        self.op.retire(result_fd, buffer_pool);
    }
}

impl std::ops::Deref for TakenOp {
    type Target = PendingOp;
    fn deref(&self) -> &PendingOp {
        &self.op
    }
}

impl std::ops::DerefMut for TakenOp {
    fn deref_mut(&mut self) -> &mut PendingOp {
        &mut self.op
    }
}

/// What a completion found when it looked its submission up.
pub(crate) enum Taken {
    /// The operation, with a fiber waiting for its result. Cook it.
    Live(TakenOp),
    /// The operation, with nobody to receive it. Retire it instead: a caller
    /// dropped its record of the submission and said so, so there is nothing
    /// left to build a result for.
    Cancelled(TakenOp),
    /// The operation, with the fiber that asked for it in a terminal state.
    /// Retire it as a cancelled one is — but answer, rather than fall silent.
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
    /// The answer is an error built from nothing the entry held. Assembling the
    /// real one instead would put values into a completion whose only remaining
    /// reference is this entry's hold, and disposing of the entry is what lets
    /// that hold go (src/io/AGENTS.md § "An operation whose fiber is gone has
    /// no reader").
    Orphaned(TakenOp),
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
    /// Ids [`orphaned_to_stop`](Self::orphaned_to_stop) has already reported, so
    /// a backend that sweeps on every drain asks each worker once. Unlike
    /// `cancelled` this changes nothing about the answer: the completion these
    /// ids are waiting for is still delivered.
    stop_asked: HashSet<SubmissionId>,
}

impl PendingTable {
    pub(crate) fn new() -> Self {
        PendingTable::default()
    }

    /// File a fresh submission's entry under the id it was dispatched with,
    /// retaining the regions its operands live in.
    pub(crate) fn insert(&mut self, id: SubmissionId, op: PendingOp, submitter: Submitter) {
        let hold = OperandHold::take(&op, submitter);
        self.ops.insert(
            id,
            Entry {
                op,
                submitter,
                hold,
            },
        );
    }

    /// Take the entry a completion resolves through, and say whether anybody
    /// is waiting for its result.
    ///
    /// Two ways to have no reader, and they are answered differently because a
    /// different amount of bookkeeping is left. The id was cancelled — a caller
    /// dropped its record of the submission and said so, so there is nothing
    /// left to tell. Or the fiber that asked has reached a terminal state
    /// without anybody cancelling for it, and the scheduler is still holding
    /// the pairing (see [`Taken::Orphaned`]).
    ///
    /// The operation comes out still holding its operands, so the completion
    /// this take feeds may read them.
    pub(crate) fn take(&mut self, id: SubmissionId) -> Taken {
        let was_cancelled = self.cancelled.remove(&id);
        self.stop_asked.remove(&id);
        let Some(e) = self.ops.remove(&id) else {
            return Taken::Unknown;
        };
        let orphaned = e.submitter.asker_finished();
        let taken = TakenOp {
            op: e.op,
            submitter: e.submitter,
            hold: e.hold,
        };
        if was_cancelled {
            Taken::Cancelled(taken)
        } else if orphaned {
            Taken::Orphaned(taken)
        } else {
            Taken::Live(taken)
        }
    }

    /// The in-flight ids whose asking fiber has reached a terminal state and
    /// whose operation nobody has asked to stop yet.
    ///
    /// An operation that parks completes when something outside this process
    /// acts, and the fiber that would have read the result is what went away,
    /// so the backend ends these itself (src/io/AGENTS.md § "Ending an
    /// operation whose fiber is gone"). Reporting an id records it here, so a
    /// caller sweeping on every drain asks each worker once; the record leaves
    /// with the entry in [`take`](Self::take).
    pub(crate) fn orphaned_to_stop(&mut self) -> Vec<SubmissionId> {
        let PendingTable {
            ops, stop_asked, ..
        } = self;
        let ids: Vec<SubmissionId> = ops
            .iter()
            .filter(|(id, e)| !stop_asked.contains(*id) && e.submitter.asker_finished())
            .map(|(id, _)| *id)
            .collect();
        stop_asked.extend(ids.iter().copied());
        ids
    }

    /// Let go of every hold still in the table, without cooking anything.
    ///
    /// Backend teardown: the operations left here will never complete, so
    /// nothing else will dispose of their entries. This runs while the store is
    /// still live — a heap quiesces every backend it carries before its region
    /// sweep — and is idempotent, so the `Drop` that calls it a second time from
    /// inside that sweep reaches nothing.
    pub(crate) fn release_holds(&mut self) {
        for e in self.ops.values_mut() {
            e.hold.release();
        }
    }

    /// What a completion for an operation whose fiber is gone says.
    ///
    /// Built from nothing the entry held — that is the point — so it names the
    /// reason rather than the operation. The only reader is the scheduler,
    /// which retires the pairing this id belongs to and drops the error: the
    /// fiber that would have received it is what went away.
    pub(crate) fn orphaned_asker_error(
        id: SubmissionId,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> crate::io::Completion {
        crate::io::Completion::err(
            id,
            crate::io::io_error(
                "io-error",
                format!(
                    "io completion {id}: the fiber that requested this operation \
                     ended before it finished, so its result reaches nobody"
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
    /// Its hold moves with it rather than being released and taken again: the
    /// operands do not change, and letting go in between would free them
    /// between two syscalls of one operation.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn restore(&mut self, id: SubmissionId, taken: TakenOp) {
        self.ops.insert(
            id,
            Entry {
                op: taken.op,
                submitter: taken.submitter,
                hold: taken.hold,
            },
        );
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::pool::BufferPool;
    use crate::value::fiber::FiberStatus;
    use crate::value::heap::HeapObject;

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

    /// A submission with neither a store nor a fiber: a `Sleep` holds no values
    /// to retain, and no fiber means nothing is ever withheld or swept.
    fn detached() -> Submitter {
        Submitter::detached(std::ptr::null_mut())
    }

    /// An entry holding one heap value. `WatchNext` is the shape with a single
    /// operand and nothing else to stand up.
    fn watch_op(pool: &mut BufferPool, watcher: Value) -> PendingOp {
        PendingOp::WatchNext {
            watcher,
            buffer_handle: pool.alloc(0),
        }
    }

    use crate::value::fiber::test_fiber_in_region as fiber_in;

    /// A heap, a region on it, and a value born in that region.
    fn value_in_fresh_region() -> (
        *mut crate::value::fiberheap::FiberHeap,
        crate::hir::region::RuntimeRegion,
        Value,
    ) {
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let region = h.new_runtime_region();
        let value = h.alloc_in_region(
            HeapObject::LBox {
                cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
                traits: Value::NIL,
            },
            region,
        );
        (heap, region, value)
    }

    /// An uncancelled submission's completion is handed its entry to cook.
    #[test]
    fn a_live_submission_is_taken_live() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), detached());
        assert!(matches!(table.take(id(1)), Taken::Live(_)));
        assert!(table.is_empty(), "taking an entry removes it");
    }

    /// A cancelled submission's completion is told so, and only once: the mark
    /// leaves with the entry, so nothing about this id survives to affect a
    /// later lookup.
    #[test]
    fn a_cancelled_submission_is_taken_cancelled_exactly_once() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool), detached());
        table.mark_cancelled(id(1));
        match table.take(id(1)) {
            Taken::Cancelled(op) => op.retire(0, &mut pool),
            _ => panic!("a marked submission must be reported cancelled"),
        }
        assert!(matches!(table.take(id(1)), Taken::Unknown));
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
        table.insert(id(1), sleep_op(&mut pool), detached());
        assert!(matches!(table.take(id(1)), Taken::Live(_)));

        table.mark_cancelled(id(1));
        assert!(matches!(table.take(id(1)), Taken::Unknown));
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
        table.insert(id(1), sleep_op(&mut pool), detached());
        let op = match table.take(id(1)) {
            Taken::Live(op) => op,
            _ => panic!("an unmarked entry is live"),
        };
        table.restore(id(1), op);

        table.mark_cancelled(id(1));
        match table.take(id(1)) {
            Taken::Cancelled(op) => op.retire(0, &mut pool),
            _ => panic!("a resubmitted operation must still be cancellable"),
        }
    }

    /// A submitted operation's operands outlive the release of the region they
    /// were born in.
    ///
    /// The trap: the entry holds `Value`s, and a `Value` is a bare pointer that
    /// keeps nothing alive. Nothing else counts a reference held by the pending
    /// table, so without the entry's own retain the region goes on the release
    /// below and the completion assembles a result out of freed memory.
    ///
    /// The counter-factual: with the retain removed, `region_generation` moves,
    /// which is the store recording that this incarnation of the region is gone.
    #[test]
    fn a_held_operand_survives_the_release_of_its_region() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let (heap, region, watcher) = value_in_fresh_region();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let born = h.region_generation(region.get());

        table.insert(
            id(1),
            watch_op(&mut pool, watcher),
            Submitter::detached(heap),
        );
        h.decref_region(region);

        assert_eq!(
            h.region_generation(region.get()),
            born,
            "the submitted operation's hold must outlast its fiber's release",
        );

        // And the hold is what was holding it: taking the entry lets go, and the
        // region is then reclaimed by the reference the release already dropped.
        match table.take(id(1)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("a detached submission is never withheld"),
        }
        assert_ne!(
            h.region_generation(region.get()),
            born,
            "disposing of the entry must let the region go",
        );
    }

    /// An operand that lives in an `Owned` region is held through the region
    /// whose count reclamation actually listens to.
    ///
    /// The trap: `incref` on an `Owned` region is a no-op by construction — that
    /// region has no count left, and its owner's subtree drop frees it however
    /// many references point at it. A hold that retained the operand's own
    /// region would compile, run, and hold nothing.
    ///
    /// The counter-factual: retain the operand's own region instead of its root,
    /// and the owner's release below frees the subtree — `region_generation`
    /// moves for the member, and the completion assembles from freed memory.
    /// `tests/elle/process-io.lisp` § 10 is the program that gets there: a
    /// connection accepted inside a process, adopted into the per-connection
    /// `ev/spawn`'s subtree, written to by a `handle-io-forward` submission.
    #[test]
    fn an_owned_operand_is_held_through_its_reclamation_root() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let (heap, member, watcher) = value_in_fresh_region();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        // The owner: a Counted region that adopts the operand's, exactly as an
        // activation adopts what it captures.
        let owner = h.new_runtime_region();
        h.adopt_region(owner, member);
        let born = h.region_generation(member.get());

        table.insert(
            id(1),
            watch_op(&mut pool, watcher),
            Submitter::detached(heap),
        );
        // The owner's last reference goes while the operation is in flight. Only
        // a count on the owner can stop the subtree drop taking the member.
        h.decref_region(owner);

        assert_eq!(
            h.region_generation(member.get()),
            born,
            "the operand's owner was released under a submitted operation — a \
             retain on the operand's own region cannot hold an Owned member",
        );

        match table.take(id(1)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("a detached submission is never withheld"),
        }
        assert_ne!(
            h.region_generation(member.get()),
            born,
            "disposing of the entry must let the subtree go",
        );
    }

    /// The fiber a submission names outlives the release of the region it was
    /// born in.
    ///
    /// The trap: the fiber is the one held value that is read on every drain
    /// rather than once at the completion — `asker_finished` dereferences it
    /// each time the backend sweeps. A fiber value is a bare pointer like any
    /// other, so the region it lives in going while its operation is still in
    /// flight makes that sweep a read of freed memory. The check that exists to
    /// notice a fiber is gone is the last place that may assume it is there.
    ///
    /// Counter-factual: with the fiber left out of `OperandHold::take`, the
    /// release below frees its region, and `tests/integration/fixtures/
    /// region-fiber-abort-io-protect-uaf.lisp` faults under `--trace=guardfree`
    /// — a fiber aborted mid-`ev/sleep` is exactly this shape.
    #[test]
    fn a_held_fiber_survives_the_release_of_its_region() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let (fiber, _handle) = fiber_in(h, FiberStatus::Paused);
        let region = crate::value::arena::region_of(h, fiber).expect("the fiber has a region");
        let born = h.region_generation(region.get());

        table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
        h.decref_region(region);

        assert_eq!(
            h.region_generation(region.get()),
            born,
            "the fiber's region went while the operation it asked for was still \
             in flight — every sweep from here reads freed memory",
        );

        // Still readable, which is the whole point of holding it.
        assert!(
            table.orphaned_to_stop().is_empty(),
            "a fiber still parked has not finished with its operation",
        );
        match table.take(id(1)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("a fiber still parked has a reader"),
        }
        assert_ne!(
            h.region_generation(region.get()),
            born,
            "disposing of the entry must let the fiber's region go",
        );
    }

    /// A resubmission keeps its hold rather than dropping and retaking it.
    ///
    /// The trap: a read that needs another syscall goes out through `take` and
    /// back through `restore`. If the take released, the operands would be
    /// unheld between the two — and once the asking fiber has gone, that hold is
    /// the last reference, so the port would be freed between two syscalls of
    /// one read.
    #[test]
    fn a_restored_operation_still_holds_its_operands() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let (heap, region, watcher) = value_in_fresh_region();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let born = h.region_generation(region.get());

        table.insert(
            id(1),
            watch_op(&mut pool, watcher),
            Submitter::detached(heap),
        );
        h.decref_region(region);
        let op = match table.take(id(1)) {
            Taken::Live(op) => op,
            _ => panic!("a detached submission is never withheld"),
        };
        table.restore(id(1), op);

        assert_eq!(
            h.region_generation(region.get()),
            born,
            "a resubmitted operation must still hold its operands",
        );
        match table.take(id(1)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("a detached submission is never withheld"),
        }
    }

    /// Teardown lets go of every hold at once, for operations that will never
    /// complete and so will never be disposed of by a completion.
    #[test]
    fn releasing_the_holds_lets_every_operand_region_go() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let (heap, region, watcher) = value_in_fresh_region();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let born = h.region_generation(region.get());

        table.insert(
            id(1),
            watch_op(&mut pool, watcher),
            Submitter::detached(heap),
        );
        h.decref_region(region);
        table.release_holds();

        assert_ne!(
            h.region_generation(region.get()),
            born,
            "teardown must let go of what the entries were holding",
        );
        // Idempotent: the second release names no store and reaches nothing.
        table.release_holds();
    }

    /// An entry whose asking fiber has reached a terminal state has no reader,
    /// whether or not anybody cancelled it.
    ///
    /// The trap: nothing marks this id. The fiber that asked ran to a terminal
    /// state by a path that told no one, so the only evidence left is the fiber
    /// itself.
    #[test]
    fn an_entry_whose_fiber_ended_is_taken_orphaned() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let (fiber, _handle) = fiber_in(h, FiberStatus::Error);

        table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
        match table.take(id(1)) {
            Taken::Orphaned(op) => op.retire(0, &mut pool),
            _ => panic!("an operation whose fiber ended has no reader"),
        }
    }

    /// The same entry read while its fiber is still running is live.
    ///
    /// The counter-factual for the test above: without it, a check that simply
    /// answered "gone" would pass that one and withhold every completion in the
    /// process.
    #[test]
    fn an_entry_whose_fiber_still_runs_is_taken_live() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let (fiber, _handle) = fiber_in(h, FiberStatus::Paused);

        table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
        match table.take(id(1)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("an operation whose fiber is still parked has a reader"),
        }
    }

    /// A fiber unwinding after `fiber/abort` is `:paused`, and its operation is
    /// left alone.
    ///
    /// The trap: an abort resumes the fiber to unwind, and that unwinding can
    /// suspend and be resumed again, so the fiber still has a result to come
    /// back for. Ending its operation then is the same red as never ending an
    /// orphaned one, from the opposite cause.
    #[test]
    fn an_unwinding_fiber_keeps_its_operation() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let (fiber, handle) = fiber_in(h, FiberStatus::Paused);

        table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
        assert!(
            table.orphaned_to_stop().is_empty(),
            "a fiber still unwinding has not finished with its operation",
        );

        // It finishes, and only then is the operation ended.
        handle.with_mut(|f| f.status = FiberStatus::Error);
        assert_eq!(table.orphaned_to_stop(), vec![id(1)]);
    }

    /// A table holding one entry whose asking fiber has ended, and one whose
    /// submission names no fiber at all.
    fn table_with_an_ended_fiber() -> (
        BufferPool,
        PendingTable,
        *mut crate::value::fiberheap::FiberHeap,
    ) {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        let heap = crate::value::arena::leaked_test_heap();
        // SAFETY: the heap is leaked for the process.
        let h = unsafe { &mut *heap };
        let (fiber, _handle) = fiber_in(h, FiberStatus::Dead);

        table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
        // A second entry nobody is waiting on as a fiber, so a sweep reporting
        // everything is distinguishable from one reporting what it should.
        table.insert(id(2), sleep_op(&mut pool), Submitter::detached(heap));
        (pool, table, heap)
    }

    /// An operation whose fiber has ended is reported for a stop exactly once,
    /// and only that operation is.
    ///
    /// The trap: a backend sweeps on every drain, which is every tick of the
    /// event loop. Without the record, each tick would ask the same worker
    /// again for an operation whose completion is already on its way.
    #[test]
    fn an_operation_whose_fiber_ended_is_reported_for_a_stop_once() {
        let (_pool, mut table, _heap) = table_with_an_ended_fiber();

        assert_eq!(
            table.orphaned_to_stop(),
            vec![id(1)],
            "only the entry whose fiber ended needs stopping",
        );
        assert!(
            table.orphaned_to_stop().is_empty(),
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
    fn an_operation_asked_to_stop_is_still_taken_orphaned() {
        let (mut pool, mut table, _heap) = table_with_an_ended_fiber();
        assert_eq!(table.orphaned_to_stop(), vec![id(1)]);

        match table.take(id(1)) {
            Taken::Orphaned(op) => op.retire(0, &mut pool),
            _ => panic!("an operation asked to stop must still answer"),
        }
    }

    /// A submission made on behalf of no fiber is never withheld and never
    /// swept. `handle-io-forward` submits for a child scheduler, whose reader is
    /// a queue rather than a fiber here.
    #[test]
    fn a_submission_with_no_fiber_is_never_withheld() {
        let (mut pool, mut table, _heap) = table_with_an_ended_fiber();
        assert_eq!(
            table.orphaned_to_stop(),
            vec![id(1)],
            "the detached submission is not swept",
        );
        match table.take(id(2)) {
            Taken::Live(op) => op.retire(0, &mut pool),
            _ => panic!("a submission naming no fiber always has a reader"),
        }
    }

    /// Teardown withholds every result at once.
    #[test]
    fn cancel_all_marks_everything_in_flight() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        for n in 1..=3 {
            table.insert(id(n), sleep_op(&mut pool), detached());
        }
        table.cancel_all();
        for n in 1..=3 {
            assert!(
                matches!(table.take(id(n)), Taken::Cancelled(_)),
                "submission {n} must be withheld at teardown",
            );
        }
    }
}
