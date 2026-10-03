// audited: 2026-09-30
//! Channel primitives — crossbeam-channel wrappers for inter-fiber messaging.
//!
//! docs/threads.md
//! docs/io/timeout.md
//!
//! A parked `chan/select` waits on the wake fds in src/primitives/chan/wake.rs.

use crate::primitives::def::RegionEffect;
use std::cell::RefCell;
use std::sync::Arc;

use crossbeam_channel::{self, TryRecvError, TrySendError};

use crate::io::request::{IoOp, IoRequest};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

mod prims;
mod wake;
use prims::*;
use wake::make_wake_fd;
pub use wake::{ChanSelectGuard, ChanSelectGuardCell, WakeList};

/// Newtype wrapper to satisfy crossbeam's `Send` requirement.
///
/// `Value` contains `Rc` (not `Send`). For single-threaded schedulers
/// (the common case) this is trivially safe. For cross-thread use the
/// scheduler is responsible for only sending immutable data.
pub(crate) struct SendableValue(Value);

impl SendableValue {
    /// Wrap a Value for cross-thread channel transport. Callers must
    /// honor the `Send` contract below — `sys/spawn`'s completion
    /// sentinel is an immediate integer (no heap), which is trivially
    /// safe to move across threads.
    pub(crate) fn new(v: Value) -> Self {
        SendableValue(v)
    }
}

// SAFETY: The scheduler contract guarantees that values sent through
// channels are either immutable or will not be accessed from the
// sending side after the send.
unsafe impl Send for SendableValue {}

/// Sender half of a channel, wrapped for `Value::external`.
///
/// Field 0 is the crossbeam sender (Optional so `chan/close` can drop
/// it without dropping the whole external).  Field 1 is the shared
/// `WakeList` — the same Arc lives in this channel's receiver half so
/// `chan/send` can wake any parked `chan/select`.
pub(crate) struct ChanSender(
    pub(crate) RefCell<Option<crossbeam_channel::Sender<SendableValue>>>,
    pub(crate) Arc<WakeList>,
);

/// Receiver half of a channel, wrapped for `Value::external`.
///
/// Field 1 is the same shared `WakeList` carried by every matching
/// sender — see `ChanSender`.
pub(crate) struct ChanReceiver(
    pub(crate) RefCell<Option<crossbeam_channel::Receiver<SendableValue>>>,
    pub(crate) Arc<WakeList>,
);

/// Clone the crossbeam sender and the shared `WakeList` from a sender
/// Value.  Returns None if the value is not a `chan/sender` or its
/// crossbeam half is already closed.
pub(crate) fn clone_sender(
    v: &Value,
) -> Option<(crossbeam_channel::Sender<SendableValue>, Arc<WakeList>)> {
    let cs = v.as_external::<ChanSender>()?;
    let tx = cs.0.borrow().as_ref().cloned()?;
    Some((tx, Arc::clone(&cs.1)))
}

/// Clone the crossbeam receiver and the shared `WakeList` from a
/// receiver Value.  Returns None if the value is not a `chan/receiver`
/// or its crossbeam half is already closed.
pub(crate) fn clone_receiver(
    v: &Value,
) -> Option<(crossbeam_channel::Receiver<SendableValue>, Arc<WakeList>)> {
    let cr = v.as_external::<ChanReceiver>()?;
    let rx = cr.0.borrow().as_ref().cloned()?;
    Some((rx, Arc::clone(&cr.1)))
}

/// Create a chan/sender Value from a raw crossbeam sender and its
/// shared `WakeList`.  The `WakeList` must be the same Arc that backs
/// the matching receiver(s).
pub(crate) fn sender_value(
    tx: crossbeam_channel::Sender<SendableValue>,
    wake: Arc<WakeList>,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Value {
    ctx.external("chan/sender", ChanSender(RefCell::new(Some(tx)), wake))
}

/// Create a chan/receiver Value from a raw crossbeam receiver and its
/// shared `WakeList`.
pub(crate) fn receiver_value(
    rx: crossbeam_channel::Receiver<SendableValue>,
    wake: Arc<WakeList>,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Value {
    ctx.external("chan/receiver", ChanReceiver(RefCell::new(Some(rx)), wake))
}

primitive! {
    "chan" => prim_chan_new {
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Create a channel. Returns [sender receiver]. Optional capacity for bounded channel.",
        params: &["&opt capacity"],
        category: "chan",
        example: "(chan)",
        aliases: &["chan/new"],
        effect: RegionEffect::Fresh,
    }
    "chan/send" => prim_chan_send {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Non-blocking send. Returns [:ok], [:full], or [:disconnected].",
        params: &["sender", "msg"],
        category: "chan",
        example: "(chan/send sender 42)",
        // `Sends`, not `Stores`: the message (arg 1) crosses to the receiving
        // fiber (by pointer — `prim_chan_send` enqueues the raw `SendableValue`).
        // The store is seam-counted (`retain_sent_message` on a successful
        // enqueue, lowered by `release_received_message` at the receive), so the
        // solver records no edge; the fiber-frontier escape of the message is the
        // escape analysis's fiber/send facet (`hir::escape`), the Shared seed the
        // ownership forest reads.
        effect: RegionEffect::Sends { args: &[1] },
    }
    "chan/recv" => prim_chan_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking receive. Returns [:ok msg], [:empty], or [:disconnected].",
        params: &["receiver"],
        category: "chan",
        example: "(chan/recv receiver)",
        effect: RegionEffect::Fresh,
    }
    "chan/clone" => prim_chan_clone {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Clone a sender. Multiple senders can feed the same channel.",
        params: &["sender"],
        category: "chan",
        example: "(chan/clone sender)",
        effect: RegionEffect::Fresh,
    }
    "chan/close" => prim_chan_close {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a sender. Receivers will get :disconnected after buffered messages drain.",
        params: &["sender"],
        category: "chan",
        example: "(chan/close sender)",
        effect: RegionEffect::Immediate,
    }
    "chan/close-recv" => prim_chan_close_recv {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a receiver. Senders will get :disconnected on next send.",
        params: &["receiver"],
        category: "chan",
        example: "(chan/close-recv receiver)",
        effect: RegionEffect::Immediate,
    }
    "chan/try-select" => prim_chan_try_select {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Non-blocking poll over receivers. Returns [index msg], [:empty], or [:disconnected].",
        params: &["receivers"],
        category: "chan",
        example: "(chan/try-select @[r1 r2])",
        effect: RegionEffect::Fresh,
    }
    "chan/wait-ready" => prim_chan_wait_ready {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Park the current fiber until a receiver is ready, a sender closes, or its :timeout or :deadline passes. Returns nil; caller re-checks with chan/try-select.",
        params: &["receivers"],
        category: "chan",
        example: "(chan/wait-ready @[r1 r2] :timeout 1)",
        // Fresh: the SIG_OK fast path builds a fresh `[:ready i v]`/`[:disconnected]`
        // array in this call's ctx region (oracle-CHECKED, same reception shape as
        // chan/recv); the yield path (ChanSelectPark) resumes with Value::NIL,
        // which Fresh permits. Stores nothing.
        effect: RegionEffect::Fresh,
    }
}
