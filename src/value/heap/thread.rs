// audited: 2026-10-22
//! `ThreadHandle`: what a spawned thread's handle holds, and the `WorkerOutcome` it reports.
//!
//! docs/threads.md

use std::sync::{Arc, Mutex};

/// How a spawned thread ended, as its handle reports it (docs/threads.md).
pub enum WorkerOutcome {
    /// The closure answered a value, serialized for the joiner's heap.
    Ready(crate::value::send::SendBundle),
    /// The closure raised, or the thread panicked: the message the join
    /// raises as `:thread-error`.
    Failed(String),
    /// The kill switch was pulled before the worker ended, and the join
    /// raises `:killed`.
    Killed,
}

/// The slot a worker fills once, and its handle reads: empty while the
/// thread runs.
pub type OutcomeSlot = Arc<Mutex<Option<WorkerOutcome>>>;

/// Thread handle for concurrent execution.
///
/// Holds the outcome of a spawned thread's execution, the kill switch that
/// ends it, and a completion channel the joiner waits on. `Arc<Mutex<>>`
/// shares the outcome across threads; the worker fills it before signalling
/// completion.
///
/// Completion is delivered via the channel wake protocol (`done_rx` /
/// `done_wake`) rather than polling: after storing its outcome the worker
/// sends a sentinel on `done_rx`'s channel and signals `done_wake`, so a
/// joiner parked in `chan/select` wakes exactly once and yields to the
/// scheduler in the meantime. `sys/thread-state` peeks `result` first (a
/// finished thread needs no wait) and otherwise hands back a fresh
/// `chan/receiver` over `done_rx` for the caller to select on. These are
/// plain `Send` fields (like `result`) — not heap `Value`s — so they carry
/// no region edge.
#[derive(Clone)]
pub struct ThreadHandle {
    /// The outcome of the spawned thread's execution.
    pub result: OutcomeSlot,
    /// Receiver half of the worker's completion channel. Cloned into a
    /// fresh `chan/receiver` Value on demand (`sys/thread-state`).
    /// `pub(crate)`: `SendableValue` is a crate-private type.
    pub(crate) done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
    /// Shared wake list for the completion channel — the worker signals
    /// it so a parked `chan/select` over `done_rx` wakes.
    pub(crate) done_wake: Arc<crate::primitives::chan::WakeList>,
    /// The spawner's end of the worker's kill switch, which `sys/kill` pulls
    /// (docs/threads.md).
    pub(crate) kill: crate::value::fiber::KillSwitch,
}

impl ThreadHandle {
    /// Create a new thread handle with a shared outcome slot, the
    /// receiver/wake-list halves of its completion channel, and the
    /// spawner's end of the worker's kill switch.
    /// `pub(crate)`: takes the crate-private `SendableValue` channel type.
    pub(crate) fn new(
        result: OutcomeSlot,
        done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
        done_wake: Arc<crate::primitives::chan::WakeList>,
        kill: crate::value::fiber::KillSwitch,
    ) -> Self {
        ThreadHandle {
            result,
            done_rx,
            done_wake,
            kill,
        }
    }
}

impl std::fmt::Debug for ThreadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ThreadHandle")
    }
}

impl PartialEq for ThreadHandle {
    fn eq(&self, _other: &Self) -> bool {
        false // Thread handles are never equal
    }
}
