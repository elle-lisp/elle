//! audited: 2026-11-20
//! What a completion finds when it looks its submission up, and the one answer a result nobody reads gets.
//!
//! docs/impl/io/cancel.md
//! docs/impl/io/inflight.md

use super::hold::TakenOp;
use crate::io::pool::BufferPool;
use crate::io::types::{FdState, PortKey};
use crate::io::{Birthplace, Completion, SubmissionId};
use std::collections::HashMap;

/// What a completion found when it looked its submission up.
pub(crate) enum Taken {
    /// The operation, with a fiber waiting for its result. Cook it.
    Live(TakenOp),
    /// The operation, whose result nobody will read. Retire it, and answer
    /// with what [`Unread::answer`] builds.
    Unread(TakenOp, Unread),
    /// The operation, put at rest by its backend's teardown. Retire it and
    /// answer nothing: nobody is left to read an answer, and the heap one would
    /// be built on may already be gone.
    AtRest(TakenOp),
    /// No entry under this id: already reaped, or never filed.
    Unknown,
}

impl Taken {
    /// The operation this take found, whatever becomes of it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn op(&self) -> Option<&TakenOp> {
        match self {
            Taken::Live(op) | Taken::Unread(op, _) | Taken::AtRest(op) => Some(op),
            Taken::Unknown => None,
        }
    }

    /// The operation, when a fiber waits for its result. Otherwise the entry is
    /// retired here, and the error carries the one answer it gives, if any.
    ///
    /// `result_code` and `data` are the raw completion's, which tell the retire
    /// what the operation obtained (see [`PendingOp::retire`]). A result nobody
    /// reads gives a remainder the read borrowed back to its port first, since
    /// the port may have another reader. A cancel gave that remainder back at
    /// the cancel, so a cancelled entry finds nothing to give and reads no
    /// port. An entry at rest gives nothing back, because its port may be gone.
    ///
    /// [`PendingOp::retire`]: super::PendingOp::retire
    pub(crate) fn live_or_retire(
        self,
        id: SubmissionId,
        result_code: i32,
        data: &[u8],
        buffer_pool: &mut BufferPool,
        fd_states: &mut HashMap<PortKey, FdState>,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<TakenOp, Option<Completion>> {
        match self {
            Taken::Live(op) => Ok(op),
            Taken::Unread(mut op, why) => {
                crate::io::landing::give_back_lent(&mut op.op, fd_states);
                op.retire(result_code, data, buffer_pool);
                Err(Some(why.answer(id, origin_heap)))
            }
            Taken::AtRest(op) => {
                op.retire(result_code, data, buffer_pool);
                Err(None)
            }
            Taken::Unknown => Err(None),
        }
    }
}

/// Why nobody will read an operation's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unread {
    /// A caller dropped its record of the submission and cancelled it.
    Cancelled,
    /// The fiber that asked reached a terminal state, and nobody cancelled for
    /// it. The scheduler still pairs the id with that fiber.
    Orphaned,
}

impl Unread {
    /// The one answer a result nobody reads gets: an error that names the
    /// reason rather than the operation.
    ///
    /// It is built from nothing the entry held, so it reads none of the values
    /// a gone fiber's release may have freed. The answer is what tells a
    /// reader that the operation has ended.
    pub(crate) fn answer(
        self,
        id: SubmissionId,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Completion {
        let birth = Birthplace::on(origin_heap);
        match self {
            Unread::Cancelled => Completion::failed(
                id,
                birth,
                "cancelled",
                format!("io completion {id}: the operation was cancelled before it finished"),
            ),
            Unread::Orphaned => Completion::failed(
                id,
                birth,
                "io-error",
                format!(
                    "io completion {id}: the fiber that requested this operation ended \
                     before it finished, so its result reaches nobody"
                ),
            ),
        }
    }
}

/// What the table records against an id in flight, beside its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    /// The sweep of gone fibers has asked this operation to stop, so it is
    /// not asked again. The answer it gets does not change.
    StopAsked,
    /// A caller cancelled this operation, and its entry let go of what the
    /// completion reads.
    Cancelled,
    /// The backend's teardown put this entry at rest.
    AtRest,
}
