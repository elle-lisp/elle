// audited: 2026-11-20
//! One in-flight operation: the shapes it can take, and the heap values it
//! holds.
//!
//! docs/impl/io/inflight.md

use super::FileStage;
use crate::io::pool::BufferHandle;
use crate::io::request::{Bound, ConnectAddr, FileKind, PortOp};
use crate::io::types::PortKey;
use crate::port::PortKind;
use crate::value::Value;
use std::os::unix::io::{OwnedFd, RawFd};
use std::rc::Rc;

/// What kind of operation a worker ran, reported beside the id so the entry
/// that id resolves through can be checked against it
/// (docs/impl/io/inflight.md).
///
/// Coarser than [`PendingOp`]: these name what a worker can report having done,
/// which is why the check is [`PendingOp::accepts`] and not an equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpKind {
    /// Stream or socket I/O on a descriptor a port owns.
    Port,
    Connect,
    Sleep,
    ProcessWait,
    Open,
    File,
    Resolve,
    Watch,
    Signal,
    /// A readiness wait on a bare descriptor.
    Poll,
}

/// Pending async I/O operation, one variant per operation shape.
///
/// Three relationships to a port run through them. `Port` names one that
/// already exists. `Connect` and `Open` build one on completion, from a value
/// the call site pre-allocated. The rest are portless.
pub(crate) enum PendingOp {
    /// Operation on an existing port.
    Port {
        op: PortOp,
        port_key: PortKey,
        port: Value,
        /// This operation's share of the descriptor it names, held until the
        /// entry is retired (docs/impl/io/descriptor.md).
        ///
        /// `None` for an operation on a port that owns no descriptor: the
        /// stdio numbers are process-wide and outlive every `Port` that names
        /// them.
        #[allow(dead_code)] // kept alive for its Drop side effect
        descriptor: Option<Rc<OwnedFd>>,
        /// A pooled buffer on the ring: a `read-all`'s accumulation, a copy of
        /// a payload that cannot be written where it lies, a socket
        /// operation's scratch, and the placeholder a flush takes. `None` for
        /// the three reads, which land in the caller's own buffer, for a write
        /// handed the kernel by address, and for every stream operation a pool
        /// worker runs, which owns the buffers it needs.
        buffer_handle: Option<BufferHandle>,
        /// For Accept: which kind of listener (TcpListener or UnixListener).
        listener_kind: Option<PortKind>,
        /// The remainder the port handed this read, already copied into the
        /// front of the caller's buffer. Given back if the read ends without
        /// answering (docs/impl/io/bytes.md).
        lent: Vec<u8>,
        /// Bytes of this operation's payload already in place: in the caller's
        /// buffer for a read (a borrowed remainder included), or written out
        /// to the fd. Both directions resubmit from this offset, and the
        /// completion counts `filled + result_code`. Zero for ops that move
        /// no payload.
        filled: usize,
        /// The request's bound, carried so a resubmission can re-arm the
        /// `LinkTimeout` that bounds it. A payload too large for one syscall
        /// completes over several SQEs. Each of them waits no longer than the
        /// bound allows when it is armed: its own `:timeout`, cut short by the
        /// call's `:deadline` (docs/io/timeout.md).
        ///
        /// Only the io_uring backend re-arms a `LinkTimeout`; the thread pool
        /// bounds the op in the worker, so on that platform every submit site
        /// still fills this field and nothing reads it back.
        bound: Bound,
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
    /// It must live until the CQE arrives. Released in completion processing,
    /// or by `retire`.
    ProcessWait {
        buffer_handle: BufferHandle,
        handle_val: Value,             // ProcessHandle — the child this wait names
        siginfo: *mut libc::siginfo_t, // kernel fills this when child exits
        /// A clone of the handle's exit record, which the ring's completion
        /// reaps through without dereferencing `handle_val`.
        exit: crate::io::request::ExitRecord,
    },
    /// Open a file path. Creates a new port on completion.
    ///
    /// On the ring, `buffer_handle` holds the path with its NUL, which the
    /// kernel reads until the CQE arrives. On the pool, `PoolOp::Open` owns the
    /// path and the buffer is an empty placeholder
    /// (docs/impl/io/platforms.md).
    Open {
        /// The file path (for error messages).
        path: String,
        buffer_handle: BufferHandle,
        /// Pre-allocated port Value (born in the solver's region at the call site).
        port: Value,
        /// The request's bound. On the ring, a bounded open has a timer of its
        /// own, which its CQE takes out (docs/impl/io/bounds.md).
        bound: Bound,
    },
    /// An operation on a file named by its path. Portless.
    ///
    /// On the ring, `buffer_handle` holds the paths with their NULs, and
    /// `stage` what the operation keeps between its CQEs. On the pool,
    /// `PoolOp::File` owns the paths, the buffer is an empty placeholder, and
    /// the stage is `Whole` (docs/impl/io/platforms.md).
    File {
        /// What the completion builds from the result.
        kind: FileKind,
        /// The path (for error messages).
        path: String,
        buffer_handle: BufferHandle,
        stage: FileStage,
    },
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
    /// An operation on an existing port, filed with nothing transferred yet.
    ///
    /// `bound` is the request's own, carried so a resubmission re-arms the
    /// bound the first submission had.
    pub(crate) fn port(
        op: PortOp,
        port_key: PortKey,
        port: Value,
        descriptor: Option<Rc<OwnedFd>>,
        buffer_handle: Option<BufferHandle>,
        bound: Bound,
    ) -> PendingOp {
        PendingOp::Port {
            op,
            port_key,
            port,
            descriptor,
            buffer_handle,
            listener_kind: None,
            lent: Vec::new(),
            filled: 0,
            bound,
        }
    }

    /// An operation of `kind` on the file at `path`, at `stage`.
    pub(crate) fn file(
        kind: FileKind,
        path: &str,
        buffer_handle: BufferHandle,
        stage: FileStage,
    ) -> PendingOp {
        PendingOp::File {
            kind,
            path: path.to_string(),
            buffer_handle,
            stage,
        }
    }

    /// The same entry, for a read that borrowed `lent` from its port and copied
    /// it into the front of the caller's buffer, so the read starts that far in.
    pub(crate) fn lending(mut self, bytes: Vec<u8>) -> PendingOp {
        if let PendingOp::Port {
            ref mut lent,
            ref mut filled,
            ..
        } = self
        {
            *filled = bytes.len();
            *lent = bytes;
        }
        self
    }

    /// The same entry, for an accept on a listener of `kind`. The completion
    /// reads the kind to decide the socket options the connection takes, and
    /// `retire` reads it to know that the entry owns a descriptor.
    pub(crate) fn accepting(mut self, kind: Option<PortKind>) -> PendingOp {
        if let PendingOp::Port {
            ref mut listener_kind,
            ..
        } = self
        {
            *listener_kind = kind;
        }
        self
    }

    /// Get the BufferHandle, if any. `None` for a port operation that holds
    /// no pooled buffer: the three reads, and a write handed the kernel by
    /// address. `Some(handle)` for every other operation.
    pub(crate) fn buffer_handle(&self) -> Option<BufferHandle> {
        match self {
            PendingOp::Port { buffer_handle, .. } => *buffer_handle,
            PendingOp::Connect { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Sleep { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ProcessWait { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Open { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::File { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::Resolve { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::WatchNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::SigNext { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::PollFd { buffer_handle, .. } => Some(*buffer_handle),
            PendingOp::ChanSelectPark { buffer_handle, .. } => Some(*buffer_handle),
        }
    }

    /// The heap values this operation holds and a completion dereferences when
    /// it cooks a result. `OperandHold` retains exactly this list
    /// (docs/impl/io/inflight.md).
    ///
    /// The trap: this match and `PortOp::operand` are exhaustive on purpose. A
    /// variant that gains a value field and does not name it goes unretained,
    /// and its region can then be freed under the completion that reads it. The
    /// request that asked for the operation declares its values through the
    /// same `PortOp::operand`. An unused slot reads `Value::NIL`, which carries
    /// no region and retains nothing.
    pub(crate) fn operands(&self) -> [Value; MAX_OPERANDS] {
        let mut out = [Value::NIL; MAX_OPERANDS];
        match self {
            PendingOp::Port { op, port, .. } => {
                out[0] = *port;
                out[1] = op.operand();
            }
            PendingOp::Connect { port, .. } | PendingOp::Open { port, .. } => out[0] = *port,
            PendingOp::ProcessWait { handle_val, .. } => out[0] = *handle_val,
            PendingOp::WatchNext { watcher, .. } => out[0] = *watcher,
            PendingOp::SigNext { receiver, .. } => out[0] = *receiver,
            PendingOp::Sleep { .. }
            | PendingOp::File { .. }
            | PendingOp::Resolve { .. }
            | PendingOp::PollFd { .. }
            | PendingOp::ChanSelectPark { .. } => {}
        }
        out
    }

    /// The one operand the kernel or a pool worker reaches by address rather
    /// than through the completion: a read's buffer, a write's payload, a
    /// `recv-from` result whose `:data` the ring receives into. A cancel keeps
    /// this one held until the completion arrives (docs/impl/io/bytes.md). `Value::NIL`,
    /// which retains nothing, for every operation that addresses none.
    pub(crate) fn addressed(&self) -> Value {
        match self {
            PendingOp::Port { op, .. } => match op {
                PortOp::ReadLine { buffer }
                | PortOp::Read { buffer, .. }
                | PortOp::ReadExact { buffer, .. } => *buffer,
                PortOp::Write { data } => data.value(),
                PortOp::RecvFrom { result, .. } => *result,
                PortOp::ReadAll
                | PortOp::Flush
                | PortOp::Accept { .. }
                | PortOp::SendTo { .. }
                | PortOp::Shutdown { .. } => Value::NIL,
            },
            PendingOp::Connect { .. }
            | PendingOp::Sleep { .. }
            | PendingOp::ProcessWait { .. }
            | PendingOp::Open { .. }
            | PendingOp::File { .. }
            | PendingOp::Resolve { .. }
            | PendingOp::WatchNext { .. }
            | PendingOp::SigNext { .. }
            | PendingOp::PollFd { .. }
            | PendingOp::ChanSelectPark { .. } => Value::NIL,
        }
    }

    /// Could an operation of kind `kind` have filed this entry? A "no" is the
    /// table and the completion contradicting each other, and the caller
    /// withholds rather than cooks (docs/impl/io/inflight.md).
    pub(crate) fn accepts(&self, kind: OpKind) -> bool {
        matches!(
            (self, kind),
            (PendingOp::Port { .. }, OpKind::Port)
                | (PendingOp::Connect { .. }, OpKind::Connect)
                | (PendingOp::Sleep { .. }, OpKind::Sleep)
                | (PendingOp::ProcessWait { .. }, OpKind::ProcessWait)
                | (PendingOp::Open { .. }, OpKind::Open)
                | (PendingOp::File { .. }, OpKind::File)
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
            PendingOp::File { .. } => "file",
            PendingOp::Resolve { .. } => "resolve",
            PendingOp::WatchNext { .. } => "watch",
            PendingOp::SigNext { .. } => "signal",
            PendingOp::PollFd { .. } => "poll",
            PendingOp::ChanSelectPark { .. } => "channel park",
        }
    }

    pub(in crate::io) fn filled(&self) -> usize {
        match self {
            PendingOp::Port { filled, .. } => *filled,
            _ => 0,
        }
    }

    /// The request's bound, for a backend re-arming it on a resubmission or
    /// taking out an open's timer. No bound for ops that carry none.
    ///
    /// Only `io::uring::drain` calls this, so the allow is narrowed to the
    /// platforms that compile that module out rather than blanket `dead_code`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(in crate::io) fn bound(&self) -> Bound {
        match self {
            PendingOp::Port { bound, .. } | PendingOp::Open { bound, .. } => *bound,
            _ => Bound::NONE,
        }
    }

    #[allow(dead_code)]
    pub(in crate::io) fn set_filled(&mut self, val: usize) {
        if let PendingOp::Port { filled, .. } = self {
            *filled = val;
        }
    }
}

/// The most heap values one operation holds: the port it names and the one
/// buffer, payload or result struct the caller reserved for it.
/// [`PendingOp::operands`] fills the rest with `Value::NIL`.
pub(crate) const MAX_OPERANDS: usize = 2;
