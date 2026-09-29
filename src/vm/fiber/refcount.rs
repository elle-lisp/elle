// audited: 2026-09-29
//! Region refcount bookkeeping for fiber signals.
//!
//! Park-retains on terminal results, and the symmetric releases when a parked
//! signal is replaced at a resume or discarded with an unrunnable fiber. These
//! balance the
//! `find_object_cross_refs` Fiber arm's free-time cascade against the retains
//! taken while a fiber holds a `signal` value across a park.
//!
//! docs/impl/region/park.md

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};

/// Incref the region of a fiber `signal`'s value, if it lives in a region
/// (no-op for `None` and region-0 immediates). The matching decref is the
/// `signal` scan in `find_object_cross_refs`'s Fiber arm, run when the fiber's
/// heap object is freed (cascade-decref) — never an explicit release, since
/// a terminal-result fiber is read (`fiber/value`) but not resumed again.
pub(super) fn incref_signal_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    signal: &Option<(SignalBits, Value)>,
) {
    if let Some((_, v)) = signal {
        let r = crate::value::arena::region_of(heap, *v);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::TerminalSignal,
        );
    }
}

/// Take the park-retain and record the `fiber → signal` content edge for a
/// TERMINAL signal a tier's execution driver installs directly into
/// `fiber.signal` — the shared form of the VM's `with_child_fiber` step-6a
/// bookkeeping (child.rs). The symmetric release is the free-time signal scan
/// (a terminal fiber is read via `fiber/value`, not resumed) or, for a resumable
/// `:error` / re-resumed fiber, [`release_displaced_terminal_signal`] at the next
/// resume. A no-op for `None`, a NON-terminal signal (a yield value / io request,
/// whose escape retain the resume path proper governs), or an immediate payload —
/// exactly the conditions under which the park owes a retain and edge.
///
/// The WASM tier's `handle_fiber_resume` installs a fiber's parked/terminal
/// signal outside the VM's fiber driver, so it must call this to keep the
/// host-side outgoing-edge table balanced against `prim_fiber_resume`'s release
/// (pinned by `tests/elle/fiber-error-resume.lisp` under `--wasm=full`).
pub(crate) fn record_terminal_signal_park(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: Value,
    signal: &Option<(SignalBits, Value)>,
) {
    let Some((bits, v)) = signal else {
        return;
    };
    if !is_terminal_signal(*bits) {
        return;
    }
    incref_signal_region(heap, signal);
    let fiber_r = crate::value::arena::region_of(heap, fiber_value);
    let sig_r = crate::value::arena::region_of(heap, *v);
    heap.record_outgoing_edge(fiber_r, sig_r);
}

/// A terminal signal is a fiber's *result*: normal return (SIG_OK), error, or
/// halt — read later via `fiber/value`, never resumed. Yield and other
/// suspending signals are transient (the fiber runs again), so their `signal`
/// value is NOT region-pinned. Must agree with the `find_object_cross_refs` Fiber
/// arm so the park-retain and the free-time cascade-decref stay balanced.
pub(crate) fn is_terminal_signal(bits: SignalBits) -> bool {
    bits.is_empty() || bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT)
}

/// Release the one reference a DISCARDED fiber's non-terminal parked signal
/// leaves stranded in its continuation — the payload reference the emitting body
/// holds across the suspend (`EmitEscape` for a `(yield v)`/`(emit …)` value,
/// `SuspendEscape` for a yielding io request or capability-denial payload). A
/// resumed body releases it itself, past the suspend; a fiber that can never run
/// again reaches no such release, so its terminal teardown
/// (`release_fiber_owned`) and the region free path's fiber discharge
/// (`RegionStore::teardown_set`) run one here instead.
///
/// Exactly ONE reference is stranded per park, which is why one decref answers
/// for it: a yielded payload's *delivery* reference is separately consumed by the
/// resumer's release of the resume result, and a payload the body borrows rather
/// than allocates is given a body reference of its own at the `Emit`
/// (docs/impl/region/park.md § "A fiber body owns one
/// reference of every value it yields"). Distinct from
/// [`release_displaced_bodyless_payload`], which answers for the ONE payload a
/// displaced park has no body reference for; at a discard there is no install to
/// owe that release and no body to double-release against
/// (docs/impl/region/park.md).
/// A no-op for `None` or an immediate.
pub(crate) fn release_discarded_signal(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
) {
    if let Some((_, v)) = parked {
        let r = crate::value::arena::region_of(heap, v);
        crate::value::arena::decref_region(heap, r);
    }
}

/// Release a parked TERMINAL signal DISPLACED by a resume or abort install.
///
/// A terminal result parked in `fiber.signal` carries a park-retain
/// ([`incref_signal_region`]) and a recorded `fiber-region → result-region`
/// content edge, both counting on the fiber's free-time signal scan to
/// consume them — sound while "terminal ⇒ never resumed" holds. It does not
/// hold everywhere: an `:error` fiber is resumable (the restarts system), and
/// a stream driver re-resumes a source whose parked signal went terminal
/// under it. The resume installs the resume value over the parked terminal,
/// so the scan never sees it: without this release the recorded table keeps
/// the dead edge (the free-time equivalence oracle detonates on the drift),
/// and each re-park stacks another — the free cascade then over-releases the
/// payload region (the `region-fiber-park-symmetry.lisp` restart face).
///
/// A no-op for `None`, a NON-terminal parked signal (a yield value, whose escape
/// retain the resumed body consumes; a runtime-built payload, which
/// [`release_displaced_bodyless_payload`] below answers for), or an immediate payload —
/// mirroring exactly the conditions under which the park took the retain and
/// recorded the edge.
pub(crate) fn release_displaced_terminal_signal(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: Value,
    parked: Option<(SignalBits, Value)>,
) {
    let Some((bits, v)) = parked else {
        return;
    };
    if !is_terminal_signal(bits) {
        return;
    }
    let Some(sig_r) = crate::value::arena::region_of(heap, v) else {
        return;
    };
    let fiber_r = crate::value::arena::region_of(heap, fiber_value);
    heap.unrecord_outgoing_edge(fiber_r, Some(sig_r));
    crate::value::arena::decref_region(heap, Some(sig_r));
}

/// Release the reference a parked RUNTIME-BUILT payload leaves stranded when an
/// install displaces it from `fiber.signal`.
///
/// A park leaves two references on its payload's region: the **delivery**, which
/// the resumer's release of the resume result consumes, and the **body's own**,
/// released by the continuation past the suspend. Two parks have no second one,
/// because the runtime built the payload and the body never named it: a
/// capability denial's `{:error :capability-denied …}` struct, and the
/// `IoRequest` a yielding io op (`ev/sleep`, `port/read`, …) returns. No
/// `decref_point` names either region, so the reference the allocation left
/// stands. What the discard discharge ([`release_discarded_signal`]) stands in for
/// on a fiber that never runs again, this stands in for on one that does:
/// `fiber/resume`'s delivery and `fiber/abort` / `fiber/refuse`'s injected error
/// each replace the payload in the slot, and each owes it one release
/// (docs/impl/region/park.md § "A payload the RUNTIME built is released by the
/// install that displaces it").
///
/// Only the ledger says which parks those are. The classifier that built the
/// park recorded the payload (`park_denial`, `park_request`), because the slot
/// cannot tell: a denial parks under the withheld capability's bits, the bits an
/// `(emit :fs v)` of a body-allocated value parks under, and a fiber that relays
/// a child's io park with `(emit :io v)` parks the child's own `IoRequest` under
/// `SIG_IO`. Both of those are body-owned, and a release here frees the value
/// under every holder that outlives the fiber.
///
/// Call this BEFORE the install, from every site that replaces another fiber's
/// parked signal — `fiber/resume`, the abort/refuse injection, and the three
/// `FiberResume` deliveries that reach an inner fiber directly. The record lives
/// on the fiber that parked, so those three are the route a `protect`ed body's
/// park takes, and the outer fiber that only passes it on releases nothing. The
/// record is read and TAKEN under one fiber borrow and the release runs against
/// the heap afterwards, so heap mutation never overlaps fiber access; taking is
/// the receipt, so a later install cannot release the same reference. Releasing
/// only what the record bit-identically names is the other half — a record left
/// over from an earlier park no longer names what is in the slot. Only a payload
/// the record claims is dereferenced.
///
/// **Every install owes it, the resume included.** A `Fresh` io op
/// (`port/read`, `accept`) mints ONE region for the call and builds both the
/// request and the completion buffer in it, then hands that buffer back as the
/// resume value — so the resume is the one install that finds the region still
/// live. It owes the release all the same, because the two references answer to
/// different consumers: the `Fresh` mint is consumed by the release of the value
/// the suspend hands back, and the `SuspendEscape` is consumed here. Standing
/// down on a resume value sharing the region leaves the second reference with no
/// consumer at all, and the region survives with its buffer and its request —
/// one per read. `tests/elle/region-io-read-strand.lisp` bounds the rate and
/// pins that the buffer still outlives this release. A denial's resume value
/// read back out of the payload is the same case.
///
/// **In flight is no reason to wait.** An abort reaches a fiber whose request
/// the scheduler already submitted, and a `Fresh` op's completion buffer lives in
/// that very region. The pending entry increfs each value its completion reads
/// and decrefs when the entry is disposed (docs/impl/region/rules.md Rule 8, the
/// submitted-I/O-operand escape site), so the region is counted for the
/// operation's whole lifetime and this decref drops the suspend retain alone.
///
/// A no-op for a fiber with no record, a record that no longer names the parked
/// signal, or an immediate payload.
pub(crate) fn release_displaced_bodyless_payload(
    heap: &mut crate::value::fiberheap::FiberHeap,
    handle: &crate::value::fiber::FiberHandle,
) {
    let displaced = handle.with_mut(|fiber| {
        let record = fiber.delivery.take_bodyless()?;
        let (_, payload) = fiber.signal?;
        record.bit_identical(payload).then_some(payload)
    });
    let Some(payload) = displaced else {
        return;
    };
    let region = crate::value::arena::region_of(heap, payload);
    crate::value::arena::decref_region(heap, region);
}

/// Release everything a park is left with when a `squelch`/`attune` boundary
/// ends it — the one end of a park that is neither a resume nor an install
/// (docs/impl/region/park.md § "A boundary ends a park with no reader and no
/// install, so it owes both references").
///
/// Two references stand on a park's payload and each answers to a seam this exit
/// cuts. The **delivery** — the `EmitEscape` / `SuspendEscape` retain the park
/// took — is consumed by the resumer's release of the resume result, and no
/// resumer ever reads a squelched park's payload out of `fiber.signal`. The
/// second is whatever a displacing install would have owed: for a payload the
/// RUNTIME built (a yielding io op's `IoRequest`, a capability denial's struct)
/// the reference its allocation left, since no `decref_point` names its region;
/// for a body-allocated payload the body's own, which the abandoned frames'
/// release tables run instead. So this releases the delivery for every park and
/// the install's half for the two runtime-built shapes, off the same ledger
/// record the installs take, so the sites cannot come to disagree about which
/// parks are which.
///
/// Two records decide it together, because neither answers on its own. The
/// LEDGER says the park's delivery retain has no reader — a fact only the site
/// that took the retain knows, and one no reading of a signal slot recovers. The
/// enforcement site says which park this exit ends, and it must, because the
/// slot cannot be read for it: two sites reach the boundary through
/// `invoke_closure_jit`, which restores the CALLER's signal first and holds the
/// parked one in a local. Releasing on the ledger alone would need every route
/// out of a park to clear the record; comparing the two bit-wise needs no such
/// argument, and a record left over from a park some other route ended names a
/// payload this exit is not looking at (the gate
/// [`release_displaced_bodyless_payload`] makes for the same reason). Taking the
/// record is the second receipt, so two boundaries over one park release one set
/// of references.
///
/// The payload's region is resolved once, before either decref, because either
/// may be the region's last. A no-op for a fiber with no live park, and for an
/// exit whose signal the ledger does not name.
pub(crate) fn release_abandoned_park(
    heap: &mut crate::value::fiberheap::FiberHeap,
    delivery: &mut crate::value::fiber::Delivery,
    live: Option<(SignalBits, Value)>,
) {
    let Some((_, payload)) = delivery.take_undelivered() else {
        return;
    };
    if !live.is_some_and(|(_, v)| v.bit_identical(payload)) {
        return;
    }
    let region = crate::value::arena::region_of(heap, payload);
    // What the install would have owed.
    if delivery
        .take_bodyless()
        .is_some_and(|r| r.bit_identical(payload))
    {
        crate::value::arena::decref_region(heap, region);
    }
    // What the reader would have consumed.
    crate::value::arena::decref_region(heap, region);
}

#[cfg(test)]
mod tests;
