//! Fiber introspection and management primitives.
//!
//! These primitives provide access to fiber state and control flow:
//! - fiber/bits: Get signal bits from last signal
//! - fiber/mask: Get the fiber's signal mask
//! - fiber/parent: Get parent fiber or nil
//! - fiber/child: Get most recently resumed child fiber or nil
//! - fiber/propagate: Propagate caught signal preserving child chain
//! - fiber/cancel: Inject error into suspended fiber
//! - fiber?: Type predicate

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{FiberStatus, SignalBits, SIG_CANCEL, SIG_ERROR, SIG_OK, SIG_PROPAGATE};
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
                "error",
                "fiber/propagate: fiber must be errored or suspended with a signal",
            ),
        );
    }

    // Return SIG_PROPAGATE — VM will extract the child's signal and propagate
    (SIG_PROPAGATE, args[0])
}

/// (fiber/cancel fiber \[value\]) → value
///
/// Inject an error into a suspended fiber. The error is injected directly
/// into the target fiber (does not walk the child chain).
///
/// Returns SIG_CANCEL — the VM handles the cancellation.
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

    // Validate: fiber must be in a cancellable state
    let status = handle.with(|fiber| fiber.status);
    match status {
        FiberStatus::New | FiberStatus::Paused => {
            // Valid for cancel — store the error value on the fiber
            handle.with_mut(|fiber| {
                fiber.signal = Some((SIG_ERROR, args.get(1).copied().unwrap_or(Value::NIL)));
            });
        }
        FiberStatus::Alive => {
            return (
                SIG_ERROR,
                error_val("error", "fiber/cancel: cannot cancel a running fiber"),
            );
        }
        FiberStatus::Dead => {
            return (
                SIG_ERROR,
                error_val("error", "fiber/cancel: cannot cancel a completed fiber"),
            );
        }
        FiberStatus::Error => {
            return (
                SIG_ERROR,
                error_val("error", "fiber/cancel: fiber already errored"),
            );
        }
    }

    // Return SIG_CANCEL — VM will handle execution
    (SIG_CANCEL, args[0])
}

/// Declarative primitive definitions for fiber introspection and management
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fiber/bits",
        func: prim_fiber_bits,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if a value is a fiber",
        params: &["value"],
        category: "fiber",
        example: "(fiber? f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/parent",
        func: prim_fiber_parent,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Get the parent fiber, or nil if none",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/parent f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/child",
        func: prim_fiber_child,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Get the most recently resumed child fiber, or nil if none",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/child f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/propagate",
        func: prim_fiber_propagate,
        effect: Effect::yields_errors(),
        arity: Arity::Exact(1),
        doc: "Propagate a caught signal from a child fiber, preserving the child chain",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/propagate f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/cancel",
        func: prim_fiber_cancel,
        effect: Effect::errors(),
        arity: Arity::Range(1, 2),
        doc: "Inject an error into a suspended fiber. Error value defaults to nil.",
        params: &["fiber", "error?"],
        category: "fiber",
        example: "(fiber/cancel f)\n(fiber/cancel f :reason)",
        aliases: &["cancel"],
    },
];
