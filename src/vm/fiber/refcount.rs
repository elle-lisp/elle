//! Region refcount bookkeeping for fiber signals: park-retains on terminal
//! results, and the symmetric releases when a parked signal is replaced at a
//! resume or discarded with an unrunnable fiber. These balance the
//! `find_object_cross_refs` Fiber arm's free-time cascade against the retains
//! taken while a fiber holds a `signal` value across a park.

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
/// (docs/impl/region/owner.md § "Park/unpark symmetry" — "A fiber body owns one
/// reference of every value it yields"). Distinct from
/// [`release_displaced_io_request`], which answers for the ONE payload a
/// discharged park has no body reference for; at a discard there is no install to
/// owe that release and no body to double-release against
/// (docs/impl/region/owner.md § "Park/unpark symmetry").
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
/// retain the resumed body consumes; an io request, which
/// [`release_displaced_io_request`] below answers for), or an immediate payload —
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

/// Release the reference a parked CAPABILITY-DENIAL payload leaves stranded when
/// an install displaces it from `fiber.signal`.
///
/// A park leaves two references on its payload's region: the **delivery**, which
/// the resumer's release of the resume result consumes, and the **body's own**,
/// released by the continuation past the suspend. A denial has no second one —
/// the denial path builds the `{:error :capability-denied …}` struct itself, so
/// the body never names it and no `decref_point` names its region. What the
/// discard discharge ([`release_discarded_signal`]) stands in for on a fiber that
/// never runs again, this stands in for on one that does: `fiber/resume`'s
/// delivery and `fiber/abort` / `fiber/refuse`'s injected error each replace the
/// payload in the slot, and each owes it one release
/// (docs/impl/region/owner.md § "Park/unpark symmetry" — "A payload the RUNTIME
/// built is released by the install that displaces it").
///
/// Call this BEFORE the install, from every site that replaces another fiber's
/// parked signal — `fiber/resume`, the abort/refuse injection, and the three
/// `FiberResume` deliveries that reach an inner fiber directly, which is the
/// route a `protect`ed body's denial takes. The record is read and TAKEN under
/// one fiber borrow and the release runs against the heap afterwards, so heap
/// mutation never overlaps fiber access; taking is the receipt, so a later
/// install cannot release the same reference. Releasing only what the record
/// bit-identically names is the other half — a record left over from an earlier
/// park no longer names what is in the slot, and an `(emit :fs v)` under the same
/// withheld bits is a body-allocated payload this must never touch.
///
/// A resume value read back OUT of the payload shares its region and still owes
/// this release — the child's continuation releases the denied call's RESULT,
/// which the delivery's own `ResumeDelivery` mint funds. A no-op for a fiber with
/// no record, a record that no longer names the parked signal, or an immediate
/// payload.
///
/// Runs beside [`release_displaced_io_request`], never in place of it: the two
/// name disjoint payloads, this one whatever the record names and that one an
/// `IoRequest`, which a denial's struct never is. Neither needs to know whether
/// the other fired, but this one runs SECOND, because its decref may be the
/// payload's last and the io arm reads the parked value to reach its verdict.
/// This one never does: the record is compared to the slot bit-wise
/// (`bit_identical`), and only a payload the record claims is dereferenced.
///
/// The record is the delivery ledger's (`park_denial` writes it, this take
/// consumes it; docs/impl/region/owner.md § "A park names its funding in the
/// delivery ledger").
pub(crate) fn release_displaced_denial_payload(
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

/// Release the `SuspendEscape` an io op left on its IoRequest's region when an
/// install displaces that request from `fiber.signal`.
///
/// A yielding io op (`ev/sleep`, `port/read`, …) returns its `IoRequest` with
/// `SIG_IO`, whereupon the suspend adds a
/// [`SuspendEscape`](crate::value::arena::EscapeSite::SuspendEscape) retain so the
/// scheduler can read the request out of `fiber.signal`. The request is the
/// RUNTIME's value: the native built it, the body never named it, and no
/// `decref_point` names its region — so the continuation past the suspend
/// releases nothing for it and the suspend retain is what the allocation leaves
/// behind. Whatever ends the park owes that release, exactly as it does for a
/// capability denial's payload
/// (docs/impl/region/owner.md § "Park/unpark symmetry" — "A payload the RUNTIME
/// built is released by the install that displaces it"): the resume that
/// delivers a completion, and the injected error `fiber/abort` / `fiber/refuse`
/// raise at the fiber's own suspension point.
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
/// pins that the buffer still outlives this release.
///
/// **In flight is no reason to wait.** An abort reaches a fiber whose request
/// the scheduler already submitted, and a `Fresh` op's completion buffer lives in
/// that very region. The pending entry increfs each value its completion reads
/// and decrefs when the entry is disposed (docs/impl/region/rules.md Rule 8, the
/// submitted-I/O-operand escape site), so the region is counted for the
/// operation's whole lifetime and this decref drops the suspend retain alone.
///
/// Gated on `SIG_IO`. A user `(yield v)` / `(emit …)` value is **body-owned** —
/// the resumed body itself releases the reference it held across the suspend, and
/// the resumer's release of the resume result consumes the delivery reference —
/// so releasing it here would double-free. The other runtime-built park payload,
/// a capability denial's struct, parks under the withheld capability's bits and
/// so cannot be told from that `(emit …)` by its bits at all; it is released by
/// [`release_displaced_denial_payload`], off the classifier's record. The two run
/// side by side and name disjoint payloads — see the reading below.
/// A no-op for a non-io signal, an immediate / `None` value, or a region-0 value.
pub(crate) fn release_displaced_io_request(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
) {
    if let Some(region) = io_request_region(heap, parked) {
        crate::value::arena::decref_region(heap, Some(region));
    }
}

/// Release everything a park is left with when a `squelch`/`attune` boundary
/// ends it — the one end of a park that is neither a resume nor an install
/// (docs/impl/region/owner.md § "A boundary ends a park with no reader and no
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
/// the install's half for the two runtime-built shapes, reusing the readings the
/// installs share so the three sites cannot come to disagree about which parks
/// are which.
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
/// payload this exit is not looking at (the gate `release_displaced_denial_payload`
/// makes for the same reason). Taking the record is the second receipt, so two
/// boundaries over one park release one set of references.
///
/// Order matters within the payload's own accounting: the io arm dereferences
/// the parked value to reach its verdict, so it runs before either decref that
/// could be the region's last. A no-op for a fiber with no live park, and for an
/// exit whose signal the ledger does not name.
pub(crate) fn release_abandoned_park(
    heap: &mut crate::value::fiberheap::FiberHeap,
    delivery: &mut crate::value::fiber::Delivery,
    live: Option<(SignalBits, Value)>,
) {
    let Some(parked) = delivery.take_undelivered() else {
        return;
    };
    let (_, payload) = parked;
    if !live.is_some_and(|(_, v)| v.bit_identical(payload)) {
        return;
    }
    // What the install would have owed, in the two readings the installs use.
    release_displaced_io_request(heap, Some(parked));
    if let Some(recorded) = delivery
        .take_bodyless()
        .filter(|r| r.bit_identical(payload))
    {
        let region = crate::value::arena::region_of(heap, recorded);
        crate::value::arena::decref_region(heap, region);
    }
    // What the reader would have consumed.
    let region = crate::value::arena::region_of(heap, payload);
    crate::value::arena::decref_region(heap, region);
}

/// The region a park's `IoRequest` lives in, or `None` where the park owes this
/// accounting nothing — no park at all, a payload some other holder answers for,
/// or an immediate. One reading of "which parks are io parks", so the release and
/// the resume's skip below cannot come to disagree about it.
///
/// **The payload's TYPE is the reading, not the `SIG_IO` bit alone.** `:io` is a
/// withheld capability like any other, so a fiber denied `:io` parks its denial
/// struct under the same bit — and that payload belongs to the ledger record
/// (`release_displaced_denial_payload`), which is written on the fiber that was
/// denied. An install reaching a fiber that merely relays the park, the outer
/// fiber of a `protect`ed denial, finds no record there and would claim the
/// struct on the bit alone; releasing it here and at the record's own install
/// frees it under the mediator. An `IoRequest` is a value only an io primitive
/// builds, so asking for one makes the two readings name disjoint payloads
/// instead of asking every install to order them.
///
/// The bits decide before the payload is touched at all, and the region is taken
/// before the type is read. A park under other bits belongs to whoever does own
/// its accounting, and reading its payload is a deref this arm cannot justify —
/// one whose region may already be gone. `region_of` answers that case with the
/// generation stamp's stale-read panic; `as_external` would read the freed page,
/// so it goes second.
fn io_request_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
) -> Option<crate::hir::region::RuntimeRegion> {
    let (bits, value) = parked?;
    if !bits.intersects(crate::value::SIG_IO) {
        return None;
    }
    let region = crate::value::arena::region_of(heap, value)?;
    value
        .as_external::<crate::io::request::IoRequest>()
        .is_some()
        .then_some(region)
}

#[cfg(test)]
mod tests;
