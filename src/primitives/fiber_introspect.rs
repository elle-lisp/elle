//! Fiber introspection and management primitives.
//!
//! These primitives provide access to fiber state and control flow:
//! - fiber/bits: Get signal bits from last signal
//! - fiber/mask: Get the fiber's signal mask
//! - fiber/parent: Get parent fiber or nil
//! - fiber/child: Get most recently resumed child fiber or nil
//! - fiber/propagate: Propagate caught signal preserving child chain
//! - fiber/cancel (cancel): Hard-kill a fiber without unwinding
//! - fiber/abort (abort): Inject error and resume for graceful unwinding
//! - fiber?: Type predicate

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{
    FiberStatus, SignalBits, SIG_ABORT, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_TERMINAL,
};
use crate::value::types::Arity;
use crate::value::Value;

/// (fiber/bits fiber) → int
///
/// Returns the signal bits from the fiber's last signal.
/// Returns 0 if the fiber has no signal.
pub(crate) fn prim_fiber_bits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/bits", "fiber");

    let bits = handle.with(|fiber| fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(SIG_OK));
    (SIG_OK, Value::int(bits.raw() as i64))
}

/// (fiber/mask fiber) → int
///
/// Returns the fiber's signal mask.
pub(crate) fn prim_fiber_mask(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/mask", "fiber");

    let mask = handle.with(|fiber| fiber.mask);
    (SIG_OK, Value::int(mask.raw() as i64))
}

/// (fiber/parent fiber) → fiber | nil
///
/// Returns the parent fiber, or nil if the fiber has no parent
/// (or the parent has been dropped).
///
/// Resolution goes through the *weak* `parent` handle, not the cached
/// `parent_value`. The cache is a `Value` pointing at the parent's
/// `HeapObject::Fiber` in whatever region the parent lived in *at resume
/// time*; the region-based RC reclaims that region at the parent's own
/// `decref_point` (`docs/impl/region/rules.md` Rule 4), so dereferencing the cache
/// after the parent is gone reads freed pages. Resolving through the weak handle
/// keeps that pointer from being followed once the parent's region is reclaimed
/// (`tests/elle/region-fiber-resume-leak.lisp`). The weak handle upgrades iff the
/// parent's `Fiber` state is still alive *somewhere* (a live region, the
/// scheduler's tables, the VM); when it does, a fresh fiber `Value` is
/// rebuilt from the upgraded handle (same `handle.id()`, so identity is
/// preserved) into the current region — never the stale cached pointer. When
/// the parent has genuinely been dropped, return nil, exactly as documented.
pub(crate) fn prim_fiber_parent(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/parent", "fiber");

    let parent = handle.with(|fiber| fiber.parent.clone());
    match parent.and_then(|w| w.upgrade()) {
        Some(parent_handle) => (SIG_OK, ctx.fiber_from_handle(parent_handle)),
        None => (SIG_OK, Value::NIL),
    }
}

/// (fiber/child fiber) → fiber | nil
///
/// Returns the most recently resumed child fiber, or nil if none.
pub(crate) fn prim_fiber_child(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/child", "fiber");

    let child_val = handle.with(|fiber| fiber.child_value.unwrap_or(Value::NIL));
    (SIG_OK, child_val)
}

/// (fiber/propagate fiber) → suspends
///
/// Propagate a caught signal from a child fiber, preserving the child chain
/// for stack traces. The fiber must be in :error or :paused status.
///
/// Returns SIG_PROPAGATE — the VM sets parent.child = fiber and propagates
/// the fiber's signal upward.
pub(crate) fn prim_fiber_propagate(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/propagate", "fiber");

    // Validate: fiber must be in error or paused state with a signal
    let has_signal = handle.with(|fiber| {
        matches!(fiber.status, FiberStatus::Error | FiberStatus::Paused) && fiber.signal.is_some()
    });

    if !has_signal {
        return (
            SIG_ERROR,
            ctx.error(
                "internal-error",
                "fiber/propagate: fiber must be errored or paused with a signal",
            ),
        );
    }

    // Return SIG_PROPAGATE — VM will extract the child's signal and propagate
    (SIG_PROPAGATE, args[0])
}

/// (fiber/cancel fiber \[value\]) → value
///
/// Hard-kill a fiber. Sets the fiber to :error status immediately without
/// resuming it. No defer blocks run, no protect handlers execute.
/// The fiber is dead. For self-cancel (cancelling the currently running
/// fiber), returns SIG_ERROR | SIG_TERMINAL which terminates the dispatch
/// loop without unwinding.
pub(crate) fn prim_fiber_cancel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/cancel", "fiber");

    let error_value = args.get(1).copied().unwrap_or(Value::NIL);

    // try_with returns None when fiber is taken (currently executing on VM).
    // That means it's the currently running fiber — self-cancel.
    let status = match handle.try_with(|fiber| fiber.status) {
        Some(s) => s,
        None => {
            // Self-cancel: fiber is alive (taken by VM). Return terminal error
            // to kill the dispatch loop without unwinding.
            return (SIG_ERROR | SIG_TERMINAL, error_value);
        }
    };

    match status {
        FiberStatus::Alive => {
            // Fiber exists in handle but status is Alive — shouldn't happen
            // in normal operation, but handle it as self-cancel.
            (SIG_ERROR | SIG_TERMINAL, error_value)
        }
        FiberStatus::New | FiberStatus::Paused => {
            // Cancel another fiber: the hard-kill teardown sets the terminal
            // error state, consumes the parked chain, and frees everything the
            // fiber owned — its parked frames' activation owner nodes and its
            // fiber owner node (docs/impl/region/owner.md § "Owner nodes" —
            // "Fiber teardown frees everything the fiber owns").
            crate::vm::fiber::kill_fiber(ctx.heap_mut(), handle, args[0], error_value);
            (SIG_OK, error_value)
        }
        FiberStatus::Dead => (
            SIG_ERROR,
            ctx.error(
                "state-error",
                "fiber/cancel: cannot cancel a completed fiber",
            ),
        ),
        FiberStatus::Error => (
            SIG_ERROR,
            ctx.error("state-error", "fiber/cancel: fiber already errored"),
        ),
    }
}

/// (fiber/abort fiber \[value\]) → value
///
/// Install `error_value` as an error raised at a PAUSED fiber's own suspension
/// point and hand the VM the abort signal that resumes it there.
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
    // (`release_displaced_terminal_signal`); a parked non-terminal signal's
    // strand here is the dead-continuation residual (docs/impl/region/owner.md
    // § "Park/unpark symmetry"), not released blind.
    let parked = handle.with(|fiber| fiber.signal);
    crate::vm::fiber::release_displaced_terminal_signal(ctx.heap_mut(), fiber_value, parked);
    handle.with_mut(|fiber| {
        fiber.signal = Some((SIG_ERROR, error_value));
        // The park this record named is over, and the strand above is what the
        // install leaves of it — so a later resume of the fiber must not read the
        // record as a release it owes (`Fiber::denial_payload`).
        fiber.denial_payload = None;
    });
    // The DELIVERY reference. Every other install of a terminal payload into a
    // signal slot funds itself — a raise mints, a re-park mints — but this one
    // installs a payload the CALLER owns, and the caller's reference answers the
    // caller's ARGUMENT release alone. Exactly one further release fires on the
    // payload as a RESULT, and which one depends on where the injected error
    // stops: the abort's caller when the fiber's mask catches it, an in-body
    // `protect`'s resume result when the fiber catches it, the resume result of
    // whichever ancestor absorbs it when it escapes, or a replayed cleanup
    // frame's parked call when the unwinding runs one. One reference, one
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
    // The VM injects the error, resumes the fiber, and lets it unwind.
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
/// does not catch unwinds through its `defer` blocks and ends `:error`, as any
/// uncaught error would.
///
/// Only a `:paused` fiber can be refused: refusal answers a call the fiber is
/// waiting on, and no other state has one. This is the guard that separates it
/// from `fiber/abort`, which hard-kills a `:new` fiber and no-ops a `:dead` one
/// — reasonable when ending a fiber, wrong when answering a request.
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

/// Gracefully terminate a fiber by injecting an error and resuming it.
/// The fiber's error handlers (protect) and cleanup blocks (defer) will
/// execute. The fiber's final state depends on what its code does with
/// the injected error — it may die, recover, or yield.
///
/// Only works on :paused fibers (must have something to unwind).
/// Returns SIG_ABORT — the VM handles the fiber swap and execution.
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
            // Nothing to unwind — hard-kill directly (like cancel), freeing
            // anything the never-started fiber owned (its fiber owner node; a
            // :new fiber has no parked chain).
            crate::vm::fiber::kill_fiber(ctx.heap_mut(), handle, args[0], error_value);
            (SIG_OK, error_value)
        }
        FiberStatus::Alive => (
            SIG_ERROR,
            ctx.error("state-error", "fiber/abort: cannot abort a running fiber"),
        ),
        // Option A: Already completed — no-op. Matches `ev/abort`'s
        // docstring ("No-op if the fiber is already completed") and lets
        // the scheduler's `handle-abort` race harmlessly with a fiber's
        // normal termination instead of raising a state-error. Returns
        // the fiber's final value (same convention as `fiber/value`).
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

/// (fiber/caps) → set
/// (fiber/caps fiber) → set
///
/// Returns the active capabilities of the current or specified fiber as a
/// keyword set. Capabilities are `~withheld & CAP_MASK`.
///
/// 0 args: queries the current fiber via SIG_QUERY.
/// 1 arg: reads the specified fiber's withheld field directly.
pub(crate) fn prim_fiber_caps(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() {
        // 0-arg form: query current fiber via SIG_QUERY
        return (
            SIG_QUERY,
            ctx.pair(Value::keyword("fiber/caps"), Value::NIL),
        );
    }
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/caps", "fiber");

    let caps = handle.with(|fiber| crate::signals::CAP_MASK.subtract(fiber.withheld));
    let registry = crate::signals::registry::global_registry().lock().unwrap();
    let keywords = registry.bits_to_keywords(caps);
    (SIG_OK, ctx.set(keywords.into_iter().collect()))
}

// Declarative primitive definitions for fiber introspection and management
primitive! {
    "fiber/bits" => prim_fiber_bits {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the signal bits from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/bits f)",
        effect: RegionEffect::Immediate,
    }
    "fiber/mask" => prim_fiber_mask {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the fiber's signal mask",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/mask f)",
        effect: RegionEffect::Immediate,
    }
    "fiber/cancel" => prim_fiber_cancel {
        signal: Signal::of(SIG_ERROR.union(SIG_TERMINAL)),
        arity: Arity::Range(1, 2),
        doc: "Hard-kill a fiber. Sets it to :error without unwinding. No defer/protect runs. Supports self-cancel.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/cancel f)\n(fiber/cancel f :reason)",
        aliases: &["cancel"],
        // The kill parks the payload as the fiber's terminal signal and takes
        // the park-retain plus its recorded `fiber → signal` outgoing edge
        // (`kill_fiber`), so the install counts its own reference — no clique.
        // A self-cancel hands the payload back instead, so the result may live
        // in an argument's region: unbounded.
        effect: RegionEffect::Delivers { args: &[1] },
    }
    "fiber/child" => prim_fiber_child {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the most recently resumed child fiber, or nil if none",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/child f)",
        // Opaque, not Mixed: the read hands back the cached child-fiber Value the
        // resume machinery left on the argument (`with_child_fiber`), so this call
        // stores nothing — while the value it returns lives in the region the child
        // was minted in, neither the call's own nor its argument's. Unbounded
        // result, no store. Mixed would seed the argument on escape's store facet,
        // costing every branch that reads a live-in fiber here its release window
        // (docs/impl/region/effects.md § "A fiber-graph read is `Opaque`").
        effect: RegionEffect::Opaque,
    }
    "fiber/parent" => prim_fiber_parent {
        arity: Arity::Exact(1),
        doc: "Get the parent fiber, or nil if this is a top-level fiber",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/parent f)",
        // Fresh: a fresh fiber Value rebuilt into this call's region from the
        // upgraded weak parent handle, or nil. Synchronous (SIG_OK) → the Fresh
        // claim is oracle-CHECKED on every debug call.
        effect: RegionEffect::Fresh,
    }
    "fiber/propagate" => prim_fiber_propagate {
        signal: Signal::of(SIG_ERROR.union(SIG_PROPAGATE)),
        arity: Arity::Exact(1),
        doc: "Propagate a caught signal from a child fiber, preserving the child chain",
        params: &["fiber"],
        category: "fiber",
        // Opaque, not Mixed: the SIG_PROPAGATE return drives the VM to write the
        // fiber argument into the propagating fiber's own `child`/`child_value`
        // pair (`handle_fiber_propagate_signal`) — the child-chain WIRING, the
        // same write `fiber/resume`'s handler performs and does not declare. The
        // free-time walk's Fiber arm never enumerates that pair, so it holds no
        // reference and the call stores nothing; the result leaves by signal, so
        // it is unbounded. The clique is empty either way (one heap argument), so
        // what the declaration decides is escape's store facet on the argument,
        // and `Mixed` would cost every branch that names a live-in fiber here its
        // release window — `defer`'s success path first
        // (docs/impl/region/effects.md § "The child-chain WIRING is `Opaque`
        // too").
        effect: RegionEffect::Opaque,
    }
    "fiber/caps" => prim_fiber_caps {
        signal: Signal::query_errors(),
        arity: Arity::Range(0, 1),
        doc: "Get the fiber's active capabilities as a keyword set",
        params: &["fiber?"],
        category: "fiber",
        example: "(fiber/caps)\n(fiber/caps f)",
        effect: RegionEffect::Fresh,
    }
    "fiber/abort" => prim_fiber_abort {
        signal: Signal::of(SIG_ERROR.union(SIG_ABORT)),
        arity: Arity::Range(1, 2),
        doc: "Gracefully terminate a fiber by injecting an error and resuming it. Defer/protect blocks run.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/abort f)\n(fiber/abort f :reason)",
        aliases: &["abort"],
        // The injected error is installed in the fiber's signal slot and taken
        // straight back out by `do_fiber_abort`; where the fiber was never
        // started, `kill_fiber` parks it under the park-retain instead. Either
        // way the install counts its own reference — no clique. The payload
        // arrives owned by the CALLER and so with no delivery of its own, which
        // `inject_error_at_suspension` mints once for whichever consumer the
        // injected error reaches. Aborting an already-dead fiber hands back that
        // fiber's terminal value, read out of the fiber argument: unbounded.
        effect: RegionEffect::Delivers { args: &[1] },
    }
    "fiber/refuse" => prim_fiber_refuse {
        signal: Signal::of(SIG_ERROR.union(SIG_ABORT)),
        arity: Arity::Range(1, 2),
        doc: "Refuse the call a paused fiber is suspended on: raise the error at the fiber's own call site, where its protect catches it. The fiber stays alive and runs on.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/refuse f)\n(fiber/refuse f :not-permitted)",
        // Same delivery accounting as `fiber/abort`: the injected error is
        // installed in the fiber's signal slot and taken straight back out by
        // `do_fiber_abort`, and the injection mints the one delivery reference
        // its consumer releases. A refused fiber that catches at its own call
        // site is the in-body-handler route — that handler's resume result is
        // the consumer. Refusal accepts only a `:paused` fiber, so neither the
        // `kill_fiber` park nor the dead-fiber terminal-value read applies.
        effect: RegionEffect::Delivers { args: &[1] },
    }
}
