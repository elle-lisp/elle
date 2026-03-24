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

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{
    FiberStatus, SignalBits, SIG_ABORT, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_TERMINAL,
};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (fiber/bits fiber) → int
///
/// Returns the signal bits from the fiber's last signal.
/// Returns 0 if the fiber has no signal.
pub(crate) fn prim_fiber_bits(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/bits: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/bits: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let bits = handle.with(|fiber| fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(SIG_OK));
    (SIG_OK, Value::int(bits.bits() as i64))
}

/// (fiber/mask fiber) → int
///
/// Returns the fiber's signal mask.
pub(crate) fn prim_fiber_mask(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/mask: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/mask: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let mask = handle.with(|fiber| fiber.mask);
    (SIG_OK, Value::int(mask.bits() as i64))
}

/// (fiber? value) → bool
///
/// Type predicate: returns true if the value is a fiber.
pub(crate) fn prim_is_fiber(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    (SIG_OK, Value::bool(args[0].is_fiber()))
}

/// (fiber/parent fiber) → fiber | nil
///
/// Returns the parent fiber, or nil if the fiber has no parent
/// (or the parent has been dropped).
pub(crate) fn prim_fiber_parent(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/parent: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/parent: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let parent_val = handle.with(|fiber| fiber.parent_value.unwrap_or(Value::NIL));
    (SIG_OK, parent_val)
}

/// (fiber/child fiber) → fiber | nil
///
/// Returns the most recently resumed child fiber, or nil if none.
pub(crate) fn prim_fiber_child(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/child: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
pub(crate) fn prim_fiber_propagate(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/propagate: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
            error_val(
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
pub(crate) fn prim_fiber_cancel(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/cancel: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
            // Cancel another fiber: set status, store error, drop frames
            handle.with_mut(|fiber| {
                fiber.status = FiberStatus::Error;
                fiber.signal = Some((SIG_ERROR, error_value));
                fiber.suspended = None;
            });
            (SIG_OK, error_value)
        }
        FiberStatus::Dead => (
            SIG_ERROR,
            error_val(
                "state-error",
                "fiber/cancel: cannot cancel a completed fiber",
            ),
        ),
        FiberStatus::Error => (
            SIG_ERROR,
            error_val("state-error", "fiber/cancel: fiber already errored"),
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
pub(crate) fn prim_fiber_abort(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/abort: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
            // Nothing to unwind — set to error directly (like cancel)
            handle.with_mut(|fiber| {
                fiber.status = FiberStatus::Error;
                fiber.signal = Some((SIG_ERROR, error_value));
                fiber.suspended = None;
            });
            (SIG_OK, error_value)
        }
        FiberStatus::Alive => (
            SIG_ERROR,
            error_val("state-error", "fiber/abort: cannot abort a running fiber"),
        ),
        FiberStatus::Dead => (
            SIG_ERROR,
            error_val("state-error", "fiber/abort: cannot abort a completed fiber"),
        ),
        FiberStatus::Error => (
            SIG_ERROR,
            error_val("state-error", "fiber/abort: fiber already errored"),
        ),
    }
}

/// Declarative primitive definitions for fiber introspection and management
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fiber/bits",
        func: prim_fiber_bits,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the signal bits from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/bits f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/mask",
        func: prim_fiber_mask,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the fiber's signal mask",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/mask f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber?",
        func: prim_is_fiber,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a value is a fiber",
        params: &["value"],
        category: "fiber",
        example: "(fiber? f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/cancel",
        func: prim_fiber_cancel,
        signal: Signal {
            bits: SignalBits::new(SIG_ERROR.0 | SIG_TERMINAL.0),
            propagates: 0,
        },
        arity: Arity::Range(1, 2),
        doc: "Hard-kill a fiber. Sets it to :error without unwinding. No defer/protect runs. Supports self-cancel.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/cancel f)\n(fiber/cancel f :reason)",
        aliases: &["cancel"],
    },
    PrimitiveDef {
        name: "fiber/child",
        func: prim_fiber_child,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the most recently resumed child fiber, or nil if none",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/child f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/parent",
        func: prim_fiber_parent,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Get the parent fiber, or nil if this is a top-level fiber",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/parent f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/propagate",
        func: prim_fiber_propagate,
        signal: Signal {
            bits: SignalBits::new(SIG_ERROR.0 | SIG_PROPAGATE.0),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Propagate a caught signal from a child fiber, preserving the child chain",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/propagate f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/abort",
        func: prim_fiber_abort,
        signal: Signal {
            bits: SignalBits::new(SIG_ERROR.0 | SIG_ABORT.0),
            propagates: 0,
        },
        arity: Arity::Range(1, 2),
        doc: "Gracefully terminate a fiber by injecting an error and resuming it. Defer/protect blocks run.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/abort f)\n(fiber/abort f :reason)",
        aliases: &["abort"],
    },
];
