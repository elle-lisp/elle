//! audited: 2026-09-17
//! One in-flight operation: the shapes it can take, the heap values it holds,
//! and what it gives back when nobody will read its result.
//!
//! docs/impl/io-inflight.md

use crate::io::pool::{BufferHandle, BufferPool};
use crate::io::request::{ConnectAddr, PortOp};
use crate::io::types::PortKey;
use crate::port::PortKind;
use crate::value::Value;
use std::os::unix::io::{OwnedFd, RawFd};
use std::rc::Rc;
use std::time::Duration;

/// What kind of operation a worker ran, reported beside the id so the entry
/// that id resolves through can be checked against it
/// (docs/impl/io-inflight.md § "One id, one operation").
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
    Task,
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
        /// entry is retired (docs/impl/io-descriptor.md § "Descriptor
        /// retirement").
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
        handle_val: Value,             // ProcessHandle — the child this wait names
        siginfo: *mut libc::siginfo_t, // kernel fills this when child exits
        /// A clone of the handle's exit record. The status is kept here rather
        /// than read out of `handle_val`, so a retire during backend teardown
        /// records without dereferencing a value whose region may be gone.
        exit: crate::io::request::ExitRecord,
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
    /// it cooks a result. `OperandHold` retains exactly this list
    /// (docs/impl/io-inflight.md § "A submitted operation holds the values its
    /// completion reads").
    ///
    /// The trap: both matches are exhaustive on purpose. A variant that gains a
    /// value field and does not name it here goes unretained, and its region can
    /// then be freed under the completion that reads it. An unused slot reads
    /// `Value::NIL`, which carries no region and retains nothing.
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

    /// Could an operation of kind `kind` have filed this entry? A "no" is the
    /// table and the completion contradicting each other, and the caller
    /// withholds rather than cooks (docs/impl/io-inflight.md § "One id, one
    /// operation").
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

    pub(in crate::io) fn filled(&self) -> usize {
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
    pub(in crate::io) fn timeout(&self) -> Option<Duration> {
        match self {
            PendingOp::Port { timeout, .. } => *timeout,
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(in crate::io) fn set_filled(&mut self, val: usize) {
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
    ///
    /// One thing is kept rather than given back: a process wait whose `waitid`
    /// succeeded has already reaped the child (src/io/AGENTS.md § "A reap is
    /// never wasted"). This is the ring's half, where the status arrives in the
    /// `siginfo_t`; the pool's is in the worker, at the `waitpid` itself.
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
            PendingOp::ProcessWait { siginfo, exit, .. } if !siginfo.is_null() => {
                // SAFETY: allocated by `Box::into_raw` at submit; reclaimed once.
                let si = unsafe { Box::from_raw(siginfo) };
                if result_fd >= 0 {
                    // SAFETY: a non-negative result is the kernel saying it
                    // completed the `waitid` and filled this `siginfo_t`.
                    exit.keep(unsafe { crate::io::request::exit_code_from_siginfo(&si) });
                }
            }
            _ => {}
        }
    }
}

/// The most heap values one operation holds: the port it names and the one
/// buffer, payload or result struct the caller reserved for it.
/// [`PendingOp::operands`] fills the rest with `Value::NIL`.
pub(crate) const MAX_OPERANDS: usize = 2;
