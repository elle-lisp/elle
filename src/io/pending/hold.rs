//! audited: 2026-09-17
//! Who a submission is for, and the reference it holds on its operands until
//! its completion has been read.
//!
//! docs/impl/io-inflight.md

use super::op::{PendingOp, MAX_OPERANDS};
use crate::io::pool::BufferPool;
use crate::value::Value;

/// Every heap value one entry holds: its operands, plus the fiber that asked.
/// [`OperandHold`] retains all of them, because the entry reads all of them.
const HELD_VALUES: usize = MAX_OPERANDS + 1;

/// Who a submission is on behalf of: the heap its operands live on, and the
/// fiber that will read its result.
///
/// The two travel together because one submission answers for both — the heap
/// is what a retain and a release reach, and the fiber is what says whether a
/// result has anywhere to go.
#[derive(Clone, Copy)]
pub(crate) struct Submitter {
    heap: *mut crate::value::fiberheap::FiberHeap,
    fiber: Value,
}

impl Submitter {
    /// A submission a fiber of this scheduler is waiting on.
    pub(crate) fn new(heap: *mut crate::value::fiberheap::FiberHeap, fiber: Value) -> Submitter {
        Submitter { heap, fiber }
    }

    /// A submission no fiber of this scheduler is waiting on. Three reach this:
    /// one forwarded for a child scheduler, whose reader is a queue and a wake
    /// box rather than a fiber here; the WASM host's top-level path, which
    /// submits and waits inline and is its own reader; and one a test issues
    /// directly. Nothing withholds such a result and nothing sweeps it —
    /// `io/cancel` is how a reader that is not a fiber lets go.
    pub(crate) fn detached(heap: *mut crate::value::fiberheap::FiberHeap) -> Submitter {
        Submitter {
            heap,
            fiber: Value::NIL,
        }
    }

    /// A submission a test issues directly: on the leaked test heap, and on
    /// behalf of no fiber. A test that is about the fiber builds its own with
    /// [`new`](Self::new).
    #[cfg(test)]
    pub(crate) fn for_test() -> Submitter {
        Submitter::detached(crate::value::arena::leaked_test_heap())
    }

    /// The heap this submission's operands live on and its results are born on.
    pub(crate) fn heap(&self) -> *mut crate::value::fiberheap::FiberHeap {
        self.heap
    }

    /// Whether the fiber that asked has reached a terminal state, so no result
    /// can reach it. False for a detached submission — there is no fiber to
    /// have ended.
    ///
    /// `try_with` rather than `with`: a fiber currently executing on the VM has
    /// been taken out of its handle, and a borrow of one panics. Such a fiber is
    /// running, which is the opposite of terminal, so the unavailable answer and
    /// the false one are the same answer.
    pub(super) fn asker_finished(&self) -> bool {
        use crate::value::fiber::FiberStatus;
        let Some(handle) = self.fiber.as_fiber() else {
            return false;
        };
        handle
            .try_with(|f| matches!(f.status, FiberStatus::Dead | FiberStatus::Error))
            .unwrap_or(false)
    }
}

/// The reference a submitted operation holds on the regions that keep its
/// operands allocated, for as long as it is in flight
/// (docs/impl/io-inflight.md § "A submitted operation holds the values its
/// completion reads").
///
/// [`release`](Self::release) is idempotent and `Drop` runs it, so an entry
/// disposed of by any route lets go exactly once.
pub(super) struct OperandHold {
    /// The store the regions below belong to. Null once released, which is what
    /// makes a second release a no-op.
    heap: *mut crate::value::fiberheap::FiberHeap,
    /// The reclamation root of each held value's region — not the region the
    /// value sits in, which for an adopted operand has no count to hold. See
    /// [`take`](Self::take).
    regions: [Option<crate::hir::region::RuntimeRegion>; HELD_VALUES],
}

impl OperandHold {
    /// Retain what keeps each of the entry's held values allocated: `op`'s
    /// operands, and the fiber that asked — which `asker_finished` dereferences
    /// on every drain, so it is held for the same reason they are.
    ///
    /// A value this store does not own retains nothing, and neither does an
    /// immediate: one has no region here to count, the other has none at all.
    pub(super) fn take(op: &PendingOp, submitter: Submitter) -> OperandHold {
        let heap = submitter.heap;
        if heap.is_null() {
            return OperandHold::released();
        }
        // SAFETY: the caller submits on the instance whose heap this is, and the
        // values were allocated on it moments ago.
        let h = unsafe { &mut *heap };
        let mut held = [Value::NIL; HELD_VALUES];
        held[..MAX_OPERANDS].copy_from_slice(&op.operands());
        held[MAX_OPERANDS] = submitter.fiber;
        let regions = held.map(|v| {
            if !h.value_in_region_store(v) {
                return None;
            }
            // The region retained is the one reclamation listens to, not the one
            // the value sits in (docs/impl/io-inflight.md § "A hold retains what
            // reclamation listens to"). `release` gives back the same root, so
            // the two are one region by construction.
            let root = crate::value::arena::region_of(h, v).map(|r| h.reclaim_root(r));
            debug_assert!(
                root.is_none_or(|r| !h.region_is_owned(r)),
                "a reclamation root is Counted by definition — an Owned one means \
                 the owner walk stopped short of the region that reclaims",
            );
            crate::value::arena::incref_for_escape(
                h,
                root,
                crate::value::arena::EscapeSite::IoSubmit,
            );
            root
        });
        OperandHold { heap, regions }
    }

    /// A hold on nothing.
    fn released() -> OperandHold {
        OperandHold {
            heap: std::ptr::null_mut(),
            regions: [None; HELD_VALUES],
        }
    }

    /// Let go of every region this hold retained. Idempotent: a released hold
    /// names no store and so reaches nothing.
    pub(super) fn release(&mut self) {
        if self.heap.is_null() {
            return;
        }
        // SAFETY: the store this hold named at the retain. Every route that
        // disposes of an entry runs while that store is live — a completion is
        // resolved on it, and the teardown release runs before the store tears
        // its regions down (`FiberHeap::quiesce_io_backends`).
        let h = unsafe { &mut *self.heap };
        for region in self.regions.iter_mut() {
            crate::value::arena::decref_region(h, region.take());
        }
        self.heap = std::ptr::null_mut();
    }
}

impl Drop for OperandHold {
    fn drop(&mut self) {
        self.release();
    }
}

/// One in-flight operation: what its completion is built from, who asked for it,
/// and the hold that keeps the values it reads.
pub(super) struct Entry {
    pub(super) op: PendingOp,
    pub(super) submitter: Submitter,
    pub(super) hold: OperandHold,
}

/// An operation taken out of the table, still holding its operands and still
/// naming who asked for it.
///
/// The hold travels with the operation rather than being let go at the take: a
/// completion reads the operands *through* the take, and the entry's hold can be
/// the last reference to them (docs/impl/io-inflight.md § "A submitted operation
/// holds the values its completion reads"). It goes when this does, or moves
/// back into the table with [`restore`](super::PendingTable::restore) — which is why the
/// submitter rides along, a resubmission being the same operation.
pub(crate) struct TakenOp {
    pub(super) op: PendingOp,
    pub(super) submitter: Submitter,
    pub(super) hold: OperandHold,
}

impl TakenOp {
    /// Give back everything this operation owns without building a value for
    /// it, then let go of its operands. See [`PendingOp::retire`].
    pub(crate) fn retire(self, result_fd: i32, buffer_pool: &mut BufferPool) {
        self.op.retire(result_fd, buffer_pool);
    }
}

impl std::ops::Deref for TakenOp {
    type Target = PendingOp;
    fn deref(&self) -> &PendingOp {
        &self.op
    }
}

impl std::ops::DerefMut for TakenOp {
    fn deref_mut(&mut self) -> &mut PendingOp {
        &mut self.op
    }
}
