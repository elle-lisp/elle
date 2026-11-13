//! audited: 2026-11-12
//! The line each port's writes wait in: one runs, and the rest wait their turn
//! out of flight, holding their operands and the copy their call was given.
//!
//! docs/impl/io/write.md

use super::hold::{OperandHold, Submitter};
use super::{PendingOp, PendingTable};
use crate::io::outgoing::Outgoing;
use crate::io::request::{Bound, PortOp};
use crate::io::types::PortKey;
use crate::io::SubmissionId;
use crate::value::Value;
use std::collections::VecDeque;
use std::time::Instant;

/// One port's writes: the one that runs, and the ones behind it in the order
/// they reached the backend.
pub(super) struct Line {
    /// The write that started last. It has left the line once its id is no
    /// longer in flight, and the next write may start then.
    running: SubmissionId,
    waiting: VecDeque<WaitingWrite>,
}

/// A write that waits for the one ahead of it on its port. It holds its
/// operands, and the copy its submission took of bytes that cannot be sent
/// where they lie; the descriptor share and any pooled buffer come when it
/// starts.
pub(crate) struct WaitingWrite {
    pub(crate) id: SubmissionId,
    pub(crate) data: Outgoing,
    /// [`Outgoing::copy_at_submission`], which the write hands its submission
    /// when it starts.
    pub(crate) copy: Option<Vec<u8>>,
    pub(crate) port: Value,
    pub(crate) port_key: PortKey,
    pub(crate) bound: Bound,
    pub(crate) submitter: Submitter,
    /// Let go of when this write leaves its line by any route. A write that
    /// starts files an entry with a hold of its own first.
    #[allow(dead_code)] // kept alive for its Drop side effect
    hold: OperandHold,
}

impl WaitingWrite {
    /// A write of `data` to `port`, with the copy its submission took,
    /// holding the regions a filed entry for the same write would hold.
    pub(crate) fn new(
        id: SubmissionId,
        data: Outgoing,
        copy: Option<Vec<u8>>,
        port: Value,
        port_key: PortKey,
        bound: Bound,
        submitter: Submitter,
    ) -> WaitingWrite {
        let op = PendingOp::port(
            PortOp::sending(data),
            port_key.clone(),
            port,
            None,
            None,
            bound,
        );
        let hold = OperandHold::take(&op, submitter);
        WaitingWrite {
            id,
            data,
            copy,
            port,
            port_key,
            bound,
            submitter,
            hold,
        }
    }
}

impl PendingTable {
    /// Whether a write to `key` must wait: one is in flight on it, or others
    /// already wait there.
    pub(crate) fn line_busy(&self, key: &PortKey) -> bool {
        self.lines
            .get(key)
            .is_some_and(|l| self.ops.contains_key(&l.running) || !l.waiting.is_empty())
    }

    /// Put a write at the back of its port's line. The caller has found the
    /// line busy.
    pub(crate) fn wait_in_line(&mut self, write: WaitingWrite) {
        let line = self
            .lines
            .get_mut(&write.port_key)
            .expect("a write waits only in a busy line");
        line.waiting.push_back(write);
    }

    /// Record that the write filed under `id` runs on `key`. The caller has
    /// found the line free, so nothing waits in it.
    pub(crate) fn runs(&mut self, key: PortKey, id: SubmissionId) {
        let line = Line {
            running: id,
            waiting: VecDeque::new(),
        };
        let before = self.lines.insert(key, line);
        debug_assert!(
            before.is_none_or(|l| l.waiting.is_empty()),
            "a write started past writes waiting on its port",
        );
    }

    /// Take the next write to start: the front of a line whose running write
    /// has left the table. It becomes that line's running write. A line with
    /// nothing left to start is dropped.
    pub(crate) fn next_waiting(&mut self) -> Option<WaitingWrite> {
        let PendingTable { ops, lines, .. } = self;
        lines.retain(|_, l| ops.contains_key(&l.running) || !l.waiting.is_empty());
        let line = lines.values_mut().find(|l| !ops.contains_key(&l.running))?;
        let next = line.waiting.pop_front()?;
        line.running = next.id;
        Some(next)
    }

    /// Take the waiting write filed under `id` out of its line, letting go of
    /// its operands. False when no write waits under `id`.
    pub(crate) fn leave_line(&mut self, id: SubmissionId) -> bool {
        self.leave_where(|w| w.id == id).len() == 1
    }

    /// Take out of their lines the waiting writes whose asking fiber has
    /// reached a terminal state, and answer their ids.
    pub(crate) fn orphaned_waiting(&mut self) -> Vec<SubmissionId> {
        self.leave_where(|w| w.submitter.asker_finished())
    }

    /// Take out of their lines the waiting writes whose deadline has passed by
    /// `now`, and answer their ids.
    pub(crate) fn expired_waiting(&mut self, now: Instant) -> Vec<SubmissionId> {
        self.leave_where(|w| w.bound.deadline().is_some_and(|d| d <= now))
    }

    /// The earliest deadline among the waiting writes. No completion may come
    /// before it passes, so a blocking wait must wake for it.
    pub(crate) fn earliest_waiting_deadline(&self) -> Option<Instant> {
        self.lines
            .values()
            .flat_map(|l| l.waiting.iter())
            .filter_map(|w| w.bound.deadline())
            .min()
    }

    /// Take every waiting write that `leaves` picks out of its line, dropping
    /// it, and answer the ids in line order.
    fn leave_where(&mut self, leaves: impl Fn(&WaitingWrite) -> bool) -> Vec<SubmissionId> {
        let mut gone = Vec::new();
        for line in self.lines.values_mut() {
            line.waiting.retain(|w| {
                let leave = leaves(w);
                if leave {
                    gone.push(w.id);
                }
                !leave
            });
        }
        gone
    }
}
