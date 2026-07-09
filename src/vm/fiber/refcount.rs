//! Region refcount bookkeeping for fiber signals: park-retains on terminal
//! results, and the symmetric releases when a parked signal is replaced or a
//! resumed fiber completes. These balance the `find_object_cross_refs` Fiber
//! arm's free-time cascade against the retains taken while a fiber holds a
//! `signal` value across a park.

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

/// Release the carrier pass-through retain `dispatch_native_call` applied when a
/// `fiber/resume` primitive returned `(SIG_RESUME, carrier)`.
///
/// `prim_fiber_resume` returns the fiber *argument* (the carrier) as its signal
/// value, so `dispatch_native_call` — which cannot tell a signal payload from a
/// real result — increfs `region_of(carrier)` (the fiber's own region) as the
/// `NativeCallResult` pass-through, expecting the caller's `DecrefValueRegion`
/// to balance it. But the resume handler REPLACES the carrier with the child's
/// actual result before pushing it, so the caller's decref targets the result's
/// region, never the carrier's. The carrier retain is left dangling and the
/// now-:dead fiber's region (dragging its closure + template) leaks
/// (`oracle.lisp`'s `fiber-resume` probe pins this reclaimed).
///
/// Release it exactly when the resumed fiber ran to completion. The child's
/// result is balanced on its own: a fresh result by its alloc reference + the
/// caller's `DecrefValueRegion`; a terminal heap result additionally by the
/// park-retain (`incref_signal_region`) + the free-time signal scan — so no
/// re-target incref of the result is needed.
///
/// Only the completion path releases. A fiber that SUSPENDED (yield / I/O)
/// stays alive and resumable; its carrier retain is the liveness hold the
/// scheduler leans on between pumps, and releasing it would free a live
/// suspended fiber (the protect/scheduler regression that sank the
/// suppress-at-dispatch attempts). So this fires solely on :dead.
pub(super) fn release_completed_resume_carrier(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: Value,
) {
    let r = crate::value::arena::region_of(heap, fiber_value);
    crate::value::arena::decref_region(heap, r);
}

/// A terminal signal is a fiber's *result*: normal return (SIG_OK), error, or
/// halt — read later via `fiber/value`, never resumed. Yield and other
/// suspending signals are transient (the fiber runs again), so their `signal`
/// value is NOT region-pinned. Must agree with the `find_object_cross_refs` Fiber
/// arm so the park-retain and the free-time cascade-decref stay balanced.
pub(crate) fn is_terminal_signal(bits: SignalBits) -> bool {
    bits.is_ok() || bits.contains(SIG_ERROR) || bits.contains(SIG_HALT)
}

/// Release the `SuspendEscape` an io op left on its IoRequest's region when a
/// parked fiber is resumed and that request — held in `fiber.signal` across the
/// park — is replaced by `resume_value`. This reclaims the IoRequest region of a
/// yielding io op — the gauge is `oracle.lisp`'s `io-yield ev/sleep` probe, which
/// dropped 5.5 → 4.5 net objects/op when this landed (the ≈4.5 residual is the
/// general escape-imprecision gap, not this mechanism).
///
/// A yielding io op (`ev/sleep`, `port/read`, …) returns its `IoRequest` with
/// `SIG_IO`, whereupon the suspend adds a
/// [`SuspendEscape`](crate::value::arena::EscapeSite::SuspendEscape) retain so the
/// scheduler can read the request out of `fiber.signal`. The request's own
/// allocation ref is consumed by the scheduler's `fiber/value` read while it
/// submits, so at resume the `SuspendEscape` is the request region's *sole*
/// remaining reference. On resume the io call "returns" `resume_value` (the
/// completion), so the caller's `DecrefValueRegion` targets THAT region, never
/// the request's — orphaning the `SuspendEscape` and leaking the request region,
/// unbounded in a long-running io loop. One decref here, the symmetric
/// counterpart of the suspend-time incref, frees it: the request is dead (the
/// scheduler already consumed it), so its region holds nothing live.
///
/// **Skip when `resume_value` shares the region** — the `Fresh` io ops
/// (`port/read`/`accept`) build their completion buffer *in* the IoRequest's
/// region and hand it back as the resume value, so that region is still live;
/// there the caller's `DecrefValueRegion` on the buffer balances the
/// `SuspendEscape`, and a decref here would free the buffer out from under the
/// caller (a use-after-free).
///
/// Gated on `SIG_IO`. A user `(yield v)` / `(emit …)` value is **body-owned** —
/// its region is released by the fiber body's own `DecrefRegion`, not by a
/// caller's `DecrefValueRegion` — so releasing it here would double-free; only an
/// io op's request is the orphaned, transient native-call result this balances.
/// A no-op for a non-io signal, an immediate / `None` value, or a region-0 value.
pub(crate) fn release_parked_signal(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
    resume_value: Value,
) {
    let Some((bits, value)) = parked else {
        return;
    };
    if !bits.contains(crate::value::SIG_IO) {
        return;
    }
    let region = crate::value::arena::region_of(heap, value);
    if region.is_none() {
        return;
    }
    // The resume value sharing the request's region is the `Fresh`-io-op signature
    // (the completion buffer is built there): that region is still live, so leave
    // it to the caller's `DecrefValueRegion`.
    if crate::value::arena::region_of(heap, resume_value) == region {
        return;
    }
    crate::value::arena::decref_region(heap, region);
}
