//! audited: 2026-11-20
//! The operations a backend has in flight, and which of them no fiber will
//! receive a result for.
//!
//! docs/impl/io/inflight.md

mod hold;
mod line;
mod op;
mod retire;
pub(crate) mod stage;
mod taken;

pub(crate) use hold::{Submitter, TakenOp};
pub(crate) use line::WaitingWrite;
pub(crate) use op::{OpKind, PendingOp};
pub(crate) use stage::FileStage;
pub(crate) use taken::{Taken, Unread};

use crate::io::types::PortKey;
use crate::io::SubmissionId;
use crate::value::Value;
use hold::{Entry, OperandHold};
use line::Line;
use std::collections::HashMap;
use taken::Mark;

/// The operations a backend has in flight, and which of them no fiber will
/// receive a result for.
///
/// The two facts live together because one decision reads both: an arriving
/// completion asks "does anybody want this?" and [`take`](Self::take) answers
/// it. So neither backend can honor the contract on one half and forget the
/// other.
///
/// A cancelled operation KEEPS its entry until its own completion arrives. The
/// worker it runs on and the descriptor it names come back with that
/// completion; dropping the entry at the cancel would strand both. What it does
/// let go of is the part of its hold the completion reads: a cancelled
/// completion is retired rather than cooked, so no operand is read again
/// (docs/impl/io/inflight.md).
#[derive(Default)]
pub(crate) struct PendingTable {
    ops: HashMap<SubmissionId, Entry>,
    /// What each marked id in flight has had done to it: asked to stop by the
    /// sweep, cancelled, or put at rest. The mark leaves with the entry.
    marks: HashMap<SubmissionId, Mark>,
    /// Each port's writes: the one running and the ones waiting behind it
    /// (docs/impl/io/write.md). A waiting write is not in flight, so none of
    /// the methods that read `ops` see it.
    lines: HashMap<PortKey, Line>,
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
    /// Two ways to have no reader, and each answers with its own error. The id
    /// was cancelled: a caller dropped its record of the submission and said
    /// so. Or the fiber that asked has reached a terminal state without anybody
    /// cancelling for it, and the scheduler is still holding the pairing. An
    /// entry the teardown put at rest answers nothing.
    ///
    /// The operation comes out still holding its operands, so the completion
    /// this take feeds may read them.
    pub(crate) fn take(&mut self, id: SubmissionId) -> Taken {
        let mark = self.marks.remove(&id);
        let Some(e) = self.ops.remove(&id) else {
            return Taken::Unknown;
        };
        let taken = TakenOp {
            op: e.op,
            submitter: e.submitter,
            hold: e.hold,
        };
        // A cancelled entry let go of its operands at the mark, the fiber that
        // asked among them, so the terminal-state question must not be asked of
        // it. A cancel answers whatever that fiber has become.
        match mark {
            Some(Mark::AtRest) => Taken::AtRest(taken),
            Some(Mark::Cancelled) => Taken::Unread(taken, Unread::Cancelled),
            _ if taken.submitter.asker_finished() => Taken::Unread(taken, Unread::Orphaned),
            _ => Taken::Live(taken),
        }
    }

    /// The in-flight ids whose asking fiber has reached a terminal state and
    /// whose operation nobody has asked to stop yet.
    ///
    /// An operation that parks completes when something outside this process
    /// acts, and the fiber that would have read the result is what went away,
    /// so the backend ends these itself (docs/impl/io/inflight.md). Reporting
    /// an id records it here, so a caller sweeping on every drain asks each
    /// worker once; the record leaves with the entry in [`take`](Self::take).
    pub(crate) fn orphaned_to_stop(&mut self) -> Vec<SubmissionId> {
        let PendingTable { ops, marks, .. } = self;
        // Any mark skips the entry. One asked to stop was reported already. A
        // cancelled one or one at rest was asked to stop by its cancel, and let
        // go of the fiber this asks about at the mark.
        let ids: Vec<SubmissionId> = ops
            .iter()
            .filter(|(id, e)| !marks.contains_key(*id) && e.submitter.asker_finished())
            .map(|(id, _)| *id)
            .collect();
        marks.extend(ids.iter().map(|id| (*id, Mark::StopAsked)));
        ids
    }

    /// Let go of every hold still in the table, without cooking anything.
    ///
    /// Backend teardown: the operations left here will never complete, so
    /// nothing else will dispose of their entries. This runs while the store is
    /// still live: a heap quiesces every backend it carries before its region
    /// sweep. It is idempotent, so the `Drop` that calls it a second time from
    /// inside that sweep reaches nothing.
    pub(crate) fn release_holds(&mut self) {
        for e in self.ops.values_mut() {
            e.hold.release();
        }
        self.lines.clear();
    }

    /// Mark `id` cancelled, so its completion is retired rather than cooked and
    /// answers `:cancelled`. Answers whether that answer will come.
    ///
    /// False for an id that is not in flight: that operation has already been
    /// reaped and its result handed on. A mark would sit in the table with no
    /// completion coming to clear it. False for an entry at rest too, which
    /// stays at rest, since the heap an answer would be built on may be gone.
    ///
    /// The entry also lets go of what its completion reads here, which is the
    /// one thing a cancel takes away from it. Nothing reads an operand again. A
    /// cancel is the ordinary path, not the rare one, so a hold kept until the
    /// completion arrives would cost every `ev/timeout` a program makes
    /// (docs/impl/io/inflight.md). The operand the kernel or a worker addresses
    /// stays held until the completion arrives, because the operation may still
    /// write into it or send from it (docs/impl/io/bytes.md).
    pub(crate) fn mark_cancelled(&mut self, id: SubmissionId) -> bool {
        let Some(e) = self.ops.get_mut(&id) else {
            return false;
        };
        if self.marks.get(&id) == Some(&Mark::AtRest) {
            return false;
        }
        e.hold.release_read();
        self.marks.insert(id, Mark::Cancelled);
        true
    }

    /// Put every operation still in flight at rest, and let go of what each
    /// completion would read. Backend teardown: the fibers are gone and the heap
    /// that carried their values may be too, so the drain that follows must
    /// retire rather than cook, must read no operand, and must build no answer.
    /// What the kernel or a worker addresses stays held until that drain
    /// retires the entry, or until `release_holds` gives up on it.
    ///
    /// Every waiting write leaves its line with nothing sent, and lets go of
    /// its operands: nothing reached either the kernel or a worker.
    pub(crate) fn cancel_all(&mut self) {
        for (id, e) in self.ops.iter_mut() {
            e.hold.release_read();
            self.marks.insert(*id, Mark::AtRest);
        }
        self.lines.clear();
    }

    /// Take back the remainder the read filed under `id` borrowed from its
    /// port, with the port's key and value, so a cancel can give it back at
    /// once. `None` when the entry borrowed nothing.
    ///
    /// The cancel calls this before it marks the id, while the entry still
    /// holds the port the give-back reads.
    pub(crate) fn take_lent(&mut self, id: SubmissionId) -> Option<(PortKey, Value, Vec<u8>)> {
        match &mut self.ops.get_mut(&id)?.op {
            PendingOp::Port {
                port_key,
                port,
                lent,
                ..
            } if !lent.is_empty() => Some((port_key.clone(), *port, std::mem::take(lent))),
            _ => None,
        }
    }

    /// Whether an entry is still filed under `id`.
    pub(crate) fn contains(&self, id: SubmissionId) -> bool {
        self.ops.contains_key(&id)
    }

    /// Put a resubmitted operation's entry back. The operation is the same one
    /// — a read that needs another syscall to reach its newline, its count, or
    /// its EOF — so this is one operation's entry moving, not a new submission.
    ///
    /// Its hold moves with it rather than being released and taken again: the
    /// operands do not change, and letting go in between would free them
    /// between two syscalls of one operation.
    ///
    /// Only the ring resubmits, so this and the two below — `len` and `ids`,
    /// which the ring's teardown loop reads — allow dead code on the
    /// platforms without a ring rather than everywhere.
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
mod tests;
