//! audited: 2026-09-23
//! The thread-pool backend: the operations a worker runs, and the one channel
//! every worker reports through.
//!
//! src/io/AGENTS.md
//! docs/impl/io-descriptor.md

use crate::io::grapheme_count_in_valid_prefix;
use crate::io::landing::{Landing, Payload};
use crate::io::pending::OpKind;
use crate::io::request::SocketOptions;
use crate::io::SubmissionId;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::time::Duration;

/// One operation a pool worker runs, typed by what its syscall needs.
///
/// A variant carries only what its syscall needs. How long the operation may
/// wait, and how `io/cancel` ends it, are not a variant's business: they arrive
/// alongside as [`Bounds`], which every `CompletionHub::submit` demands. That
/// is what makes an operation that parks without a bound unwritable rather than
/// merely discouraged.
pub(super) enum PoolOp {
    /// Read once, into the room `landing` names: the caller's buffer behind
    /// any remainder already copied there, or a buffer of the worker's own.
    Read {
        fd: RawFd,
        landing: Landing,
    },
    /// Read until the bytes hold `count` units, the stream ends, or an error
    /// fires. Units are bytes when `graphemes` is false and grapheme clusters
    /// when true — Elle strings are grapheme-counted, so a text-port
    /// `port/read-exact 50` must yield a string of `(length 50)` however many
    /// bytes that takes. On EOF before `count`, the completion answers nil.
    ReadExact {
        fd: RawFd,
        landing: Landing,
        count: usize,
        graphemes: bool,
        /// The generation that segments cluster-counted reads; captured at
        /// request build on the VM thread, applied on the worker thread.
        gen: crate::segment::Generation,
        /// A remainder the port kept rather than lending it, because the
        /// buffer could not hold it (docs/impl/io-bytes.md). It counts toward
        /// `count`, so a worker that ignored it would wait for bytes the peer
        /// has already sent; and it is the bytes rather than their length,
        /// because a cluster can straddle it and what this read returns. Empty
        /// whenever the remainder was lent, since the landing then starts with
        /// it.
        held: Vec<u8>,
    },
    /// Write every byte of `payload`, looping over short writes.
    Write {
        fd: RawFd,
        payload: Payload,
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
    /// Read into `landing` until a newline arrives, the buffer is full, or the
    /// stream ends. A full buffer with no newline is a piece of a longer line.
    ReadLine {
        fd: RawFd,
        landing: Landing,
    },
    /// Read until EOF, into an accumulation the worker owns and hands back.
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
    /// Read up to `size` bytes from `fd`, into a buffer the worker owns and
    /// hands back with the completion: the shape a test that drives a worker
    /// directly needs, with no caller's buffer behind it.
    #[cfg(test)]
    pub(super) fn read(fd: RawFd, size: usize) -> PoolOp {
        PoolOp::Read {
            fd,
            landing: Landing::owned(size),
        }
    }

    /// Write every byte of `data` to `fd`, from a buffer the worker owns.
    #[cfg(test)]
    pub(super) fn write(fd: RawFd, data: Vec<u8>) -> PoolOp {
        PoolOp::Write {
            fd,
            payload: Payload::Owned(data),
        }
    }

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

/// What a pool worker reports: the id it ran, the kind of operation, the
/// result code, and the bytes it produced.
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

mod hub;
use hub::publish_completion;
pub(super) use hub::CompletionHub;

mod stdin;
pub(super) use stdin::{StdinCompletion, StdinOpKind, StdinThread};

#[cfg(test)]
mod tests;
