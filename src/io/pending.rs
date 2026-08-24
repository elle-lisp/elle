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
    /// `result_fd` is the raw completion's result code, which for a connect or
    /// an open is the descriptor the worker opened. Nobody will take it now, so
    /// it is closed here rather than leaked.
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
            PendingOp::ProcessWait { siginfo, .. } if !siginfo.is_null() => {
                // SAFETY: allocated by `Box::into_raw` at submit; reclaimed once.
                drop(unsafe { Box::from_raw(siginfo) });
            }
            _ => {}
        }
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
    ops: HashMap<SubmissionId, PendingOp>,
    /// Ids whose result no fiber will receive. Every `io/cancel` caller in the
    /// scheduler drops its own record of the submission before cancelling, so
    /// the id is marked here precisely when there is no longer a reader.
    cancelled: HashSet<SubmissionId>,
}

impl PendingTable {
    pub(crate) fn new() -> Self {
        PendingTable::default()
    }

    /// File a fresh submission's entry under the id it was dispatched with.
    pub(crate) fn insert(&mut self, id: SubmissionId, op: PendingOp) {
        self.ops.insert(id, op);
    }

    /// Take the entry a completion resolves through, and say whether anybody
    /// is waiting for its result.
    pub(crate) fn take(&mut self, id: SubmissionId) -> Taken {
        let was_cancelled = self.cancelled.remove(&id);
        match self.ops.remove(&id) {
            Some(op) if was_cancelled => Taken::Cancelled(op),
            Some(op) => Taken::Live(op),
            None => Taken::Unknown,
        }
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
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn restore(&mut self, id: SubmissionId, op: PendingOp) {
        self.ops.insert(id, op);
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
        self.ops.get(&id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SubmissionId, &PendingOp)> {
        self.ops.iter()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &PendingOp> {
        self.ops.values()
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

    /// An uncancelled submission's completion is handed its entry to cook.
    #[test]
    fn a_live_submission_is_taken_live() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        table.insert(id(1), sleep_op(&mut pool));
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
        table.insert(id(1), sleep_op(&mut pool));
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
        table.insert(id(1), sleep_op(&mut pool));
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
        table.insert(id(1), sleep_op(&mut pool));
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

    /// Teardown withholds every result at once.
    #[test]
    fn cancel_all_marks_everything_in_flight() {
        let mut pool = BufferPool::new();
        let mut table = PendingTable::new();
        for n in 1..=3 {
            table.insert(id(n), sleep_op(&mut pool));
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
