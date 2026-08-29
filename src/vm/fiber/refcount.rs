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
/// [`release_parked_signal`], whose io gate and shared-region skip are
/// resume-path concerns: at a discard there is no resume value and no body to
/// double-release against (docs/impl/region/owner.md § "Park/unpark symmetry").
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
/// A no-op for `None`, a NON-terminal parked signal (a yield value / io
/// request, whose escape retain the resume path proper consumes — see
/// [`release_parked_signal`] below), or an immediate payload — mirroring
/// exactly the conditions under which the park took the retain and recorded
/// the edge.
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

/// Release the one reference a park with **no body reference** leaves over when
/// the resume replaces its payload with `resume_value`.
///
/// A park's payload carries two references, and the resume consumes both: the
/// delivery — the escape retain — which the resumer's release of the resume
/// result takes, and the suspending body's own, released by the continuation past
/// the suspend. Two parks have no body reference for that second consumer to
/// take, so one is left over at every resume and one decref here answers for it
/// (docs/impl/region/owner.md § "A park with no body reference owes one release at
/// the resume"). A user `(yield v)` / `(emit …)` value is body-owned and must not
/// reach the decref: releasing it double-frees the payload under every holder that
/// outlives the fiber.
///
/// **A yielding io op** (`ev/sleep`, `port/read`, …) returns its `IoRequest` with
/// `SIG_IO`, whereupon the suspend adds a
/// [`SuspendEscape`](crate::value::arena::EscapeSite::SuspendEscape) retain so the
/// scheduler can read the request out of `fiber.signal`. The request's own
/// allocation ref is consumed by the scheduler's `fiber/value` read while it
/// submits, so at resume the `SuspendEscape` is the request region's *sole*
/// remaining reference. On resume the io call "returns" `resume_value` (the
/// completion), so the caller's `DecrefValueRegion` targets THAT region, never
/// the request's — orphaning the `SuspendEscape` and leaking the request region,
/// unbounded in a long-running io loop. The gauge is `oracle.lisp`'s `io-yield
/// ev/sleep` probe, which measures the whole suspend/pump/resume round bounded.
///
/// **Skip when `resume_value` shares the region** — the `Fresh` io ops
/// (`port/read`/`accept`) build their completion buffer *in* the IoRequest's
/// region and hand it back as the resume value, so that region is still live;
/// there the caller's `DecrefValueRegion` on the buffer balances the
/// `SuspendEscape`, and a decref here would free the buffer out from under the
/// caller (a use-after-free). The skip is the io arm's alone: a denial payload's
/// region is the denied call's own, and the mediating parent's resume value comes
/// from outside the denied fiber entirely.
///
/// **A capability denial** parks the payload the VM builds in place of a call it
/// refuses to run. The denied primitive never returns, so the replayed frame's
/// result release targets `resume_value` and the payload's birth reference is the
/// one left over. Only the denial site knows a park has that shape, so it records
/// the payload in `denial_payload` ([`crate::value::fiber::Fiber::denial_payload`])
/// and the decref is gated on that record still naming the parked value.
///
/// A no-op for a park of neither shape, an immediate / `None` value, or a
/// region-0 value.
pub(crate) fn release_parked_signal(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
    denial_payload: Option<Value>,
    resume_value: Value,
) {
    let Some((bits, value)) = parked else {
        return;
    };
    let region = crate::value::arena::region_of(heap, value);
    if region.is_none() {
        return;
    }
    if bits.intersects(crate::value::SIG_IO) {
        // The resume value sharing the request's region is the `Fresh`-io-op
        // signature (the completion buffer is built there): that region is still
        // live, so leave it to the caller's `DecrefValueRegion`.
        if crate::value::arena::region_of(heap, resume_value) == region {
            return;
        }
    } else if !denial_payload.is_some_and(|p| p.bit_identical(value)) {
        return;
    }
    crate::value::arena::decref_region(heap, region);
}

#[cfg(test)]
mod tests;
