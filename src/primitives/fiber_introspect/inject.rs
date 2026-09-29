// audited: 2026-09-28
//! `fiber/abort` and `fiber/refuse`: raise an error at a paused fiber's own
//! suspension point, through one shared injection.
//!
//! docs/signals/primitives.md
//! docs/impl/region/park.md

use crate::value::fiber::{FiberStatus, SignalBits, SIG_ABORT, SIG_ERROR, SIG_OK};
use crate::value::Value;

/// (fiber/abort fiber \[value\]) → value
///
/// Install `error_value` as an error raised at a PAUSED fiber's own suspension
/// point and hand the VM the abort signal that raises it there.
///
/// Shared by `fiber/abort` and `fiber/refuse`. The two differ in intent and in
/// which fiber states they accept, not in the injection: both raise the error
/// where the fiber stopped, so the fiber's `protect` and `defer` see it. Keeping
/// one body means the region bookkeeping below cannot drift between them.
fn inject_error_at_suspension(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    fiber_value: Value,
    handle: &crate::value::fiber::FiberHandle,
    error_value: Value,
) -> (SignalBits, Value) {
    // A parked TERMINAL result this install displaces carries a park-retain +
    // recorded content edge the free-time signal scan will never see
    // (`release_displaced_terminal_signal`). A parked non-terminal signal is
    // released only where the RUNTIME built its payload (below); a body-allocated
    // one keeps its body reference, which the parked frames' own owed-release
    // tables claim (docs/impl/region/park.md).
    let parked = handle.with(|fiber| fiber.signal);
    crate::vm::fiber::release_displaced_terminal_signal(ctx.heap_mut(), fiber_value, parked);
    // A park whose payload the RUNTIME built is what this install does answer
    // for: the child's continuation releases nothing for such a value, and
    // raising at the suspension point displaces it exactly as a resume would
    // (docs/impl/region/park.md § "A payload the
    // RUNTIME built is released by the install that displaces it"). Two parks
    // are that shape and each has its own reading — a capability denial's
    // payload by the classifier's record, a yielding io op's `IoRequest` by the
    // payload's own type — so the two name disjoint payloads and both run. The
    // io arm goes first because it is the one that READS the parked value to
    // decide, and the denial arm's release may have been the payload's last.
    // The resume's `Fresh`-op skip does not travel here: an injected error is
    // not a delivery, so an error value living in the request's region owes this
    // release all the same.
    crate::vm::fiber::release_displaced_io_request(ctx.heap_mut(), parked);
    crate::vm::fiber::release_displaced_denial_payload(ctx.heap_mut(), handle);
    handle.with_mut(|fiber| {
        fiber.signal = Some((SIG_ERROR, error_value));
        // The park the payload-named records described is over, and the strand
        // above is what the install leaves of it — so a later resume of the
        // fiber must not read a record as a release it owes.
        fiber.delivery.displace();
    });
    // The DELIVERY reference. Every other install of a terminal payload into a
    // signal slot funds itself — a raise mints, a re-park mints — but this one
    // installs a payload the CALLER owns, and the caller's reference answers the
    // caller's ARGUMENT release alone. Exactly one further release fires on the
    // payload as a RESULT, and which one depends on where the injected error
    // stops: the abort's caller when the fiber's mask catches it, an in-body
    // `protect`'s resume result when the fiber catches it, the resume result of
    // whichever ancestor absorbs it when it escapes, or the parked call of a
    // `protect`/`defer` caller's frame, replayed once its sub-fiber takes the
    // error. One reference, one
    // consumer, four routes — minting here, at the seam all four leave through,
    // is what keeps any of them from having to recognize itself
    // (docs/impl/region/effects.md § `Delivers`;
    // `tests/elle/region-fiber-abort-delivery-uaf.lisp` carries a face per
    // route). `region_of` no-ops an immediate payload.
    let heap = ctx.heap_mut();
    let region = crate::value::arena::region_of(heap, error_value);
    crate::value::arena::incref_for_escape(
        heap,
        region,
        crate::value::arena::EscapeSite::AbortDelivery,
    );
    // The VM raises the error at the fiber's suspension point.
    (SIG_ABORT, fiber_value)
}

/// (fiber/refuse fiber) → value
/// (fiber/refuse fiber error) → value
///
/// Refuse the call a paused fiber is suspended on: raise `error` as a failure
/// at the fiber's own call site, so its `protect` or `try` catches it there.
///
/// A refusal is not a termination. A fiber that catches keeps running and may
/// be refused again on its next call — which is what a mediator needs, since a
/// refused operation is an ordinary event in a mediated session. A fiber that
/// does not catch stops `:error` at the refused call, as for any uncaught
/// error there. A resume restarts it at that call, and `fiber/cancel` ends it.
///
/// Only a `:paused` fiber can be refused: refusal answers a call the fiber is
/// waiting on, and no other state has one. This is the guard that separates it
/// from `fiber/abort`, which kills a `:new` fiber and no-ops a `:dead` one —
/// reasonable when ending a fiber, wrong when answering a request.
pub(crate) fn prim_fiber_refuse(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/refuse", "fiber");

    let error_value = args.get(1).copied().unwrap_or(Value::NIL);
    let status = handle.with(|fiber| fiber.status);

    match status {
        FiberStatus::Paused => inject_error_at_suspension(ctx, args[0], handle, error_value),
        other => (
            SIG_ERROR,
            ctx.error(
                "state-error",
                format!(
                    "fiber/refuse: expected a paused fiber, got :{}",
                    other.as_str()
                ),
            ),
        ),
    }
}

/// Raise an error at a `:paused` fiber's suspension point. The fiber's own
/// `protect` and `defer` see it there, as for any raise at that call. Where
/// nothing in the fiber catches it, the fiber stops `:error` at the call, and a
/// resume restarts it there. A `protect` that catches it may leave the fiber
/// `:dead` or `:paused` instead.
///
/// A `:new` fiber has no suspension point, so it is killed `:error` and never
/// runs. A `:dead` fiber answers its final value. Returns SIG_ABORT for a
/// `:paused` fiber — the VM raises the error and handles the fiber swap.
pub(crate) fn prim_fiber_abort(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/abort", "fiber");

    let error_value = args.get(1).copied().unwrap_or(Value::NIL);
    let status = handle.with(|fiber| fiber.status);

    match status {
        FiberStatus::Paused => inject_error_at_suspension(ctx, args[0], handle, error_value),
        FiberStatus::New => {
            // No suspension point to raise at — hard-kill directly, leaving the
            // fiber `:error` with the value, and freeing anything the
            // never-started fiber owned (its fiber owner node; a :new fiber has
            // no parked chain).
            crate::vm::fiber::kill_fiber(
                ctx.heap_mut(),
                handle,
                args[0],
                error_value,
                FiberStatus::Error,
            );
            (SIG_OK, error_value)
        }
        FiberStatus::Alive => (
            SIG_ERROR,
            ctx.error("state-error", "fiber/abort: cannot abort a running fiber"),
        ),
        // Already completed — no-op. Matches `ev/abort`'s docstring ("No-op if
        // the fiber is already completed") and lets the scheduler's
        // `handle-abort` race harmlessly with a fiber's normal termination
        // instead of raising a state-error. Returns the fiber's final value
        // (same convention as `fiber/value`).
        FiberStatus::Dead => (
            SIG_OK,
            handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL)),
        ),
        FiberStatus::Error => (
            SIG_ERROR,
            ctx.error("state-error", "fiber/abort: fiber already errored"),
        ),
    }
}
