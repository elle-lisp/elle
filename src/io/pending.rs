//! audited: 2026-09-18
//! The operations a backend has in flight, and which of them no fiber will
//! receive a result for.
//!
//! docs/impl/io-inflight.md

mod hold;
mod op;

pub(crate) use hold::{Submitter, TakenOp};
pub(crate) use op::{OpKind, PendingOp};

use crate::io::SubmissionId;
use hold::{Entry, OperandHold};
use std::collections::{HashMap, HashSet};

/// What a completion found when it looked its submission up.
pub(crate) enum Taken {
    /// The operation, with a fiber waiting for its result. Cook it.
    Live(TakenOp),
    /// The operation, with nobody to receive it. Retire it instead: a caller
    /// dropped its record of the submission and said so, so there is nothing
    /// left to build a result for.
    Cancelled(TakenOp),
    /// The operation, with the fiber that asked for it in a terminal state.
    /// Retire it as a cancelled one is — but answer, rather than fall silent,
    /// and answer with an error built from nothing the entry held
    /// (docs/impl/io-inflight.md § "An operation whose fiber is gone has no
    /// reader").
    Orphaned(TakenOp),
    /// No entry under this id — already reaped, or never filed.
    Unknown,
}

/// The operations a backend has in flight, and which of them no fiber will
/// receive a result for.
///
/// The two facts live together because one decision reads both: an arriving
/// completion asks "does anybody want this?" and [`take`](Self::take) answers
/// it, so neither backend can honor the contract on one half and forget the
/// other.
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
    /// so the backend ends these itself (docs/impl/io-inflight.md § "Ending an
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
        let birth = crate::io::Birthplace::on(origin_heap);
        let msg = format!(
            "io completion {id}: the fiber that requested this operation ended \
             before it finished, so its result reaches nobody"
        );
        crate::io::Completion::failed(id, birth, "io-error", msg)
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
mod tests;
