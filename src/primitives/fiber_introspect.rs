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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/bits: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/mask: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

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
/// (see `release_completed_resume_carrier`). The weak handle upgrades iff the
/// parent's `Fiber` state is still alive *somewhere* (a live region, the
/// scheduler's tables, the VM); when it does, a fresh fiber `Value` is
/// rebuilt from the upgraded handle (same `handle.id()`, so identity is
/// preserved) into the current region — never the stale cached pointer. When
/// the parent has genuinely been dropped, return nil, exactly as documented.
pub(crate) fn prim_fiber_parent(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/parent: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/child: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let child_val = handle.with(|fiber| fiber.child_value.unwrap_or(Value::NIL));
    (SIG_OK, child_val)
}

/// (fiber/propagate fiber) → suspends
///
/// Propagate a caught signal from a child fiber, preserving the child chain
/// for stack traces. The fiber must be in :error or :suspended status.
///
/// Returns SIG_PROPAGATE — the VM sets parent.child = fiber and propagates
/// the fiber's signal upward.
pub(crate) fn prim_fiber_propagate(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "fiber/propagate: expected fiber, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    // Validate: fiber must be in error or paused state with a signal
    let has_signal = handle.with(|fiber| {
        matches!(fiber.status, FiberStatus::Error | FiberStatus::Paused) && fiber.signal.is_some()
    });

    if !has_signal {
        return (
            SIG_ERROR,
            ctx.error(
                "internal-error",
                "fiber/propagate: fiber must be errored or suspended with a signal",
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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/cancel: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/abort: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let error_value = args.get(1).copied().unwrap_or(Value::NIL);
    let status = handle.with(|fiber| fiber.status);

    match status {
        FiberStatus::Paused => {
            // Store the error value on the fiber for do_fiber_abort to pick up
            handle.with_mut(|fiber| {
                fiber.signal = Some((SIG_ERROR, error_value));
            });
            // Return SIG_ABORT — VM will inject error, resume, let it unwind
            (SIG_ABORT, args[0])
        }
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
    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/caps: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

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
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_TERMINAL),
            propagates: 0,
        }),
        arity: Arity::Range(1, 2),
        doc: "Hard-kill a fiber. Sets it to :error without unwinding. No defer/protect runs. Supports self-cancel.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/cancel f)\n(fiber/cancel f :reason)",
        aliases: &["cancel"],
        effect: RegionEffect::Mixed,
    }
    "fiber/child" => prim_fiber_child {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the most recently resumed child fiber, or nil if none",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/child f)",
        effect: RegionEffect::Mixed,
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
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_PROPAGATE),
            propagates: 0,
        }),
        arity: Arity::Exact(1),
        doc: "Propagate a caught signal from a child fiber, preserving the child chain",
        params: &["fiber"],
        category: "fiber",
        // Kept Mixed (NOT PassThrough, an audit candidate): the SIG_PROPAGATE
        // return drives the VM to store the fiber cross-fiber into `parent.child`,
        // and as a single-heap-arg control-flow op its clique is empty either way
        // — a PassThrough claim would be a silent yield-path over-tighten with no
        // payoff.
        effect: RegionEffect::Mixed,
    }
    "fiber/caps" => prim_fiber_caps {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_QUERY),
            propagates: 0,
        }),
        arity: Arity::Range(0, 1),
        doc: "Get the fiber's active capabilities as a keyword set",
        params: &["fiber?"],
        category: "fiber",
        example: "(fiber/caps)\n(fiber/caps f)",
        effect: RegionEffect::Fresh,
    }
    "fiber/abort" => prim_fiber_abort {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_ABORT),
            propagates: 0,
        }),
        arity: Arity::Range(1, 2),
        doc: "Gracefully terminate a fiber by injecting an error and resuming it. Defer/protect blocks run.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/abort f)\n(fiber/abort f :reason)",
        aliases: &["abort"],
        effect: RegionEffect::Mixed,
    }
}
