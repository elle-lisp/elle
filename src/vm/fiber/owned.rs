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
    /// The releases those activations took over from their own frame-replacing
    /// tail calls (docs/impl/region/owner.md § "A deferred tail-call release has
    /// the node's life"). A frame this fiber can never re-enter never reaches
    /// the completion that would have run them.
    parked_deferred: Vec<crate::hir::region::RuntimeRegion>,
    /// The parked non-terminal signal (a yielded value / io request / denial
    /// payload), whose one park escape retain is released on the resume path —
    /// which will never come for a terminal fiber (`release_discarded_signal`).
    parked_signal: Option<(SignalBits, Value)>,
    /// The values the parked frames still owed a release for, off their own
    /// value-route slots, and the payload those releases must leave standing
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A frame that can never be re-entered never reaches the
    /// release, so it runs here.
    parked_owed: Vec<Value>,
    parked_owed_regions: Vec<crate::hir::region::MappedRegion>,
    parked_protect: Option<Value>,
    /// The fiber's own owner node (`Fiber::fiber_owner_node`).
    fiber_node: Option<crate::hir::region::RuntimeRegion>,
}

/// What the activations of an abandoned frame chain still owed: each parked
/// frame's owner node, and the releases it took over from its own
/// frame-replacing tail calls. The frames' continuations will never run, so the
/// completion release that would have discharged them never fires; it belongs to
/// whoever abandoned the chain — the discard chokepoint
/// (`VM::discard_suspended_frames`) or the terminal-fiber teardown
/// ([`take_fiber_owned`]). A `FiberResume` frame owes nothing (its sub-fiber has
/// its own lifecycle).
/// Each region here takes one tolerant decref, node and deferred region alike:
/// a node's rc goes 1→0 and subtree-drops its adopted members, and a deferred
/// region's decref is the one its emitting instruction never ran. Order between
/// them is immaterial — where one holds a counted edge to the other, the cascade
/// and the direct decref commute.
pub(crate) fn parked_activation_dues(
    frames: Vec<SuspendedFrame>,
) -> impl Iterator<Item = crate::hir::region::RuntimeRegion> {
    frames.into_iter().flat_map(|frame| match frame {
        SuspendedFrame::Bytecode(f) => {
            let dues = f.activation_dues;
            dues.deferred.into_iter().chain(dues.owner_node)
        }
        SuspendedFrame::FiberResume { .. } => Vec::new().into_iter().chain(None),
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
        parked_deferred: parked.deferred,
        parked_signal: parked.signal,
        parked_owed: parked.owed,
        parked_owed_regions: parked.owed_regions,
        parked_protect: parked.protect,
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
        parked_deferred,
        parked_signal,
        parked_owed,
        parked_owed_regions,
        parked_protect,
        fiber_node,
    } = owned;
    // The releases the parked frames still owed. Each is one reference the frame
    // took and the route that would have dropped it can no longer be reached; the
    // payload's own region is skipped, its holder being the fiber's result rather
    // than any frame.
    let protect_region = parked_protect.and_then(|v| crate::value::arena::region_of(heap, v));
    for v in parked_owed {
        let r = crate::value::arena::region_of(heap, v);
        if r.is_some() && r != protect_region {
            crate::value::arena::decref_region(heap, r);
        }
    }
    // The slot-routed half. The mapping's generation is checked first: a slot
    // whose region has since been freed and recycled is a leftover the frame's own
    // release already answered for, and releasing the id's new incarnation would
    // free a live region.
    for m in parked_owed_regions {
        if heap.generation_raw(m.region.get()) != m.gen || protect_region == Some(m.region) {
            continue;
        }
        heap.decref_region_if_present(m.region);
    }
    // The deferred releases run before the nodes, so a closure region a node's
    // member still holds a counted edge to is not freed twice — the decref here
    // drops this activation's reference and the member's edge keeps it standing
    // until the subtree drop below cascades through.
    for region in parked_deferred {
        heap.decref_region_if_present(region);
    }
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
    super::refcount::record_terminal_signal_park(heap, fiber_value, &signal);
    release_fiber_owned(heap, owned);
}
