//! Owner-node teardown for terminal fibers. A fiber owns state through the
//! ownership forest (its parked chain's activation nodes and its own fiber
//! node); when it can never run again, that state must be freed exactly once.
//! The take/release split keeps heap mutation disjoint from fiber access: the
//! take empties the fiber's slots under its borrow, and the release — run after
//! the borrow is dropped — can cascade-free the fiber's own heap value without
//! invalidating a live borrow (docs/impl/region/owner.md § "Owner nodes").

use crate::value::fiber::FiberStatus;
use crate::value::{FiberHandle, SuspendedFrame, Value, SIG_ERROR};

/// Everything a fiber owns through the ownership forest, TAKEN out of the fiber
/// (its slots emptied) so the release can run after the fiber borrow is dropped
/// (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns"). Splitting the take from the release keeps heap mutation
/// disjoint from fiber access: the release's cascades can free the fiber's own
/// heap value without invalidating a live borrow.
pub(crate) struct FiberOwned {
    /// Each still-parked `BytecodeFrame`'s activation owner node, in chain order.
    parked_nodes: Vec<crate::hir::region::RuntimeRegion>,
    /// The fiber's own owner node ([`Fiber::fiber_owner_node`]).
    fiber_node: Option<crate::hir::region::RuntimeRegion>,
}

/// The parked activation owner nodes of an abandoned frame chain. The frames'
/// continuations will never run, so the completion release that would have freed
/// each node never fires; the release belongs to whoever abandoned the chain —
/// the discard chokepoint (`VM::discard_suspended_frames`) or the terminal-fiber
/// teardown ([`take_fiber_owned`]). A `FiberResume` frame owns no node (its
/// sub-fiber has its own lifecycle).
pub(crate) fn parked_owner_nodes(
    frames: Vec<SuspendedFrame>,
) -> impl Iterator<Item = crate::hir::region::RuntimeRegion> {
    frames.into_iter().filter_map(|frame| match frame {
        SuspendedFrame::Bytecode(f) => f.activation_owner_node,
        SuspendedFrame::FiberResume { .. } => None,
    })
}

/// Take everything a TERMINAL fiber owns — the parked chain's activation owner
/// nodes and the fiber owner node — emptying the fiber's slots so no other
/// release path can reach them (the move discipline that makes the teardown the
/// sole demise). Pair with [`release_fiber_owned`] once the fiber borrow is
/// dropped. Terminal means the fiber can never be resumed: `:dead` (completion,
/// halt) or a hard kill (cancel; abort of a not-yet-started fiber). An `:error`
/// fiber is NOT terminal — it is resumable (the restarts system replays its
/// re-parked frame) — so an error promotion must never take its owned state.
pub(crate) fn take_fiber_owned(fiber: &mut crate::value::fiber::Fiber) -> FiberOwned {
    let parked_nodes = fiber
        .suspended
        .take()
        .map(|frames| parked_owner_nodes(frames).collect())
        .unwrap_or_default();
    FiberOwned {
        parked_nodes,
        fiber_node: fiber.fiber_owner_node.take(),
    }
}

/// Free everything a terminal fiber owned (the [`take_fiber_owned`] set). When a
/// fiber node exists, each parked node's members are first gathered under it
/// (`reparent_owned_children`) and the emptied node freed, so the teardown is ONE
/// set-drop over the fiber's whole owned set; with no fiber node each parked node
/// subtree-drops directly. One tolerant decref per node: rc 1→0, subtree drop over
/// node + adopted members (interior cycles reclaim with the set), the Shared
/// frontier cascading once from the recorded `outgoing` tables.
pub(crate) fn release_fiber_owned(
    heap: &mut crate::value::fiberheap::FiberHeap,
    owned: FiberOwned,
) {
    let FiberOwned {
        parked_nodes,
        fiber_node,
    } = owned;
    for node in parked_nodes {
        if let Some(fnode) = fiber_node {
            heap.reparent_owned_children(node, fnode);
        }
        heap.decref_region_if_present(node);
    }
    if let Some(fnode) = fiber_node {
        heap.decref_region_if_present(fnode);
    }
}

/// The hard-kill teardown `fiber/cancel` (of a new/parked fiber) and
/// `fiber/abort` (of a not-yet-started one) route through: set the terminal
/// error state, drop the parked chain, and free everything the fiber owned.
/// The take runs under the fiber borrow; the release after it is dropped
/// ([`take_fiber_owned`]). Unlike an ordinary `:error` promotion — which keeps
/// the fiber resumable — a hard kill consumes the chain, so nothing it owned can
/// ever be replayed.
pub(crate) fn kill_fiber(
    heap: &mut crate::value::fiberheap::FiberHeap,
    handle: &FiberHandle,
    error_value: Value,
) {
    let owned = handle.with_mut(|fiber| {
        fiber.status = FiberStatus::Error;
        fiber.signal = Some((SIG_ERROR, error_value));
        take_fiber_owned(fiber)
    });
    release_fiber_owned(heap, owned);
}
