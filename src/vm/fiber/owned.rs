//! Owner-node teardown for terminal fibers. A fiber owns state through the
//! ownership forest (its parked chain's activation nodes and its own fiber
//! node); when it can never run again, that state must be freed exactly once.
//! The take/release split keeps heap mutation disjoint from fiber access: the
//! take empties the fiber's slots under its borrow, and the release — run after
//! the borrow is dropped — can cascade-free the fiber's own heap value without
//! invalidating a live borrow (docs/impl/region/owner.md § "Owner nodes").

use crate::value::fiber::FiberStatus;
use crate::value::{FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR};

/// Everything a fiber owns through the ownership forest, TAKEN out of the fiber
/// (its slots emptied) so the release can run after the fiber borrow is dropped
/// (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns"). Splitting the take from the release keeps heap mutation
/// disjoint from fiber access: the release's cascades can free the fiber's own
/// heap value without invalidating a live borrow.
pub(crate) struct FiberOwned {
    /// Each still-parked `BytecodeFrame`'s activation owner node, in chain order.
    parked_nodes: Vec<crate::hir::region::RuntimeRegion>,
    /// The parked non-terminal signal (a yielded value / io request / denial
    /// payload), whose one park escape retain is released on the resume path —
    /// which will never come for a terminal fiber (`release_discarded_signal`).
    parked_signal: Option<(SignalBits, Value)>,
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
/// nodes, the parked non-terminal signal, and the fiber owner node — emptying
/// the fiber's slots so no other release path can reach them (the move
/// discipline that makes the teardown the sole demise). Pair with
/// [`release_fiber_owned`] once the fiber borrow is dropped. Terminal means the
/// fiber can never be resumed: `:dead` (completion, halt) or a hard kill
/// (cancel; abort of a not-yet-started fiber). An `:error` fiber is NOT
/// terminal — it is resumable (the restarts system replays its re-parked
/// frame) — so an error promotion must never take its owned state; a dropped
/// `:error` fiber discharges at its region's free instead
/// (docs/impl/region/owner.md § "The free-path fiber discharge").
pub(crate) fn take_fiber_owned(fiber: &mut crate::value::fiber::Fiber) -> FiberOwned {
    let parked = fiber.take_parked_state();
    FiberOwned {
        parked_nodes: parked.nodes,
        parked_signal: parked.signal,
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
        parked_signal,
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
    super::release_discarded_signal(heap, parked_signal);
}

/// The hard-kill teardown `fiber/cancel` (of a new/parked fiber) and
/// `fiber/abort` (of a not-yet-started one) route through: set the terminal
/// error state, drop the parked chain, and free everything the fiber owned.
/// The take runs under the fiber borrow; the release after it is dropped
/// ([`take_fiber_owned`]). Unlike an ordinary `:error` promotion — which keeps
/// the fiber resumable — a hard kill consumes the chain, so nothing it owned can
/// ever be replayed.
///
/// The kill PARKS `error_value` as the fiber's terminal signal (read later via
/// `fiber/value`), so it owes the same park-retain + recorded content edge the
/// completion path takes (`do_fiber_resume` step 6a): the fiber's free releases
/// the payload's region exactly once through the recorded edge, and the debug
/// equivalence oracle asserts the table matches the content scan. Without the
/// pair, a heap payload in a live foreign region is an unrecorded edge AND an
/// over-free at the fiber's free
/// (`runtime::tests::ownership::fnode::fiber_kill_park_retains_terminal_payload`).
/// The retain precedes [`release_fiber_owned`], whose cascade could otherwise
/// free a payload that lived in the fiber's owned set.
pub(crate) fn kill_fiber(
    heap: &mut crate::value::fiberheap::FiberHeap,
    handle: &FiberHandle,
    fiber_value: Value,
    error_value: Value,
) {
    let signal = Some((SIG_ERROR, error_value));
    let owned = handle.with_mut(|fiber| {
        // Take BEFORE installing the terminal error: the take releases the OLD
        // parked signal's SuspendEscape retain (a yielded io request / denial
        // payload the kill supersedes) — replacing first would strand it.
        let owned = take_fiber_owned(fiber);
        fiber.status = FiberStatus::Error;
        fiber.signal = signal;
        owned
    });
    super::refcount::incref_signal_region(heap, &signal);
    let fiber_r = crate::value::arena::region_of(heap, fiber_value);
    let sig_r = crate::value::arena::region_of(heap, error_value);
    heap.record_outgoing_edge(fiber_r, sig_r);
    release_fiber_owned(heap, owned);
}
