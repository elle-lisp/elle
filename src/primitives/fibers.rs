//! Fiber lifecycle primitives.
//!
//! Core fiber operations: creation, resumption, signaling, status, and
//! value extraction. Introspection and management primitives (bits, mask,
//! parent, child, propagate, cancel, fiber?) are in `fiber_introspect.rs`.
//!
//! Signal-bits resolution lives in `resolve`; fuel (instruction-budget) ops
//! live in `fuel`. Both are re-exported so the `primitive!` table below — and
//! external callers naming `crate::primitives::fibers::resolve_signal_bits` —
//! resolve the handler paths unchanged.

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{
    Fiber, FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_RESUME, SIG_YIELD,
};
use crate::value::types::Arity;
use crate::value::Value;

mod fuel;
mod resolve;

use fuel::*;
pub(crate) use resolve::resolve_signal_bits;
use resolve::status_keyword;

/// (fiber/new fn mask [:deny bits]) → fiber
///
/// Create a fiber from a closure and a signal mask. The mask determines
/// which signals the parent catches when resuming this fiber.
///
/// Optional `:deny` keyword arg withholds capabilities from the fiber.
/// The child's `withheld` is the union of the explicit deny bits and the
/// parent's withheld (propagated at resume time by the VM).
pub(crate) fn prim_fiber_new(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let closure = match args[0].as_closure() {
        Some(c) => std::rc::Rc::new(c.clone()),
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/new: expected closure, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let mask = match resolve_signal_bits(&args[1], "fiber/new", ctx) {
        Ok(bits) => bits,
        Err(err) => return err,
    };

    // Parse optional keyword arguments after the required (closure, mask) pair.
    let mut deny_bits = SignalBits::EMPTY;
    let mut i = 2;
    while i < args.len() {
        if args[i].as_keyword_name().as_deref() == Some("deny") {
            if i + 1 >= args.len() {
                return (
                    SIG_ERROR,
                    ctx.error("arity-error", "fiber/new: :deny requires a value"),
                );
            }
            deny_bits = match resolve_signal_bits(&args[i + 1], "fiber/new :deny", ctx) {
                Ok(bits) => bits,
                Err(err) => return err,
            };
            i += 2;
        } else {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    format!(
                        "fiber/new: unexpected keyword argument :{}",
                        args[i]
                            .as_keyword_name()
                            .unwrap_or_else(|| args[i].type_name().to_string())
                    ),
                ),
            );
        }
    }

    let mut fiber = Fiber::new(closure, mask);
    // The closure VALUE rides along so the first resume can install it as the
    // body's executing-closure register (see `Fiber::closure_value`).
    fiber.closure_value = args[0];
    fiber.withheld = deny_bits;
    (SIG_OK, ctx.fiber(fiber))
}

/// (fiber/resume fiber) → value
/// (fiber/resume fiber value) → value
///
/// Resume a fiber. If the fiber is New, starts execution. If Suspended,
/// delivers the value and continues from where it left off.
///
/// Returns SIG_RESUME — the VM handles the actual fiber swap.
pub(crate) fn prim_fiber_resume(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
                format!("fiber/resume: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("fiber/resume: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let resume_value = args.get(1).copied().unwrap_or(Value::NIL);

    // Validate fiber status; a New/Paused/Error fiber is resumable (Error'd
    // fibers resume via the restarts system; only Dead is terminal). Read the
    // parked signal out alongside the status: a Paused fiber that yielded an io
    // request (or any value) holds it here, and its region carries the
    // suspend-time escape references that must be balanced before the resume
    // value replaces it.
    let (status, parked) = handle.with(|fiber| (fiber.status, fiber.signal));
    match status {
        FiberStatus::New | FiberStatus::Paused | FiberStatus::Error => {
            // Release the references the now-completing yielding call left on its
            // parked value's region — otherwise every yielding io op leaks its
            // IoRequest region (see `release_parked_signal`).
            crate::vm::fiber::release_parked_signal(ctx.heap_mut(), parked, resume_value);
            handle.with_mut(|fiber| fiber.signal = Some((SIG_OK, resume_value)));
        }
        FiberStatus::Alive => {
            return (
                SIG_ERROR,
                ctx.error("state-error", "fiber/resume: fiber is already running"),
            );
        }
        FiberStatus::Dead => {
            return (
                SIG_ERROR,
                ctx.error("state-error", "fiber/resume: cannot resume completed fiber"),
            );
        }
    }

    // Return SIG_RESUME — VM will handle the fiber swap
    (SIG_RESUME, args[0])
}

/// (emit bits value) → suspends
///
/// Emit a signal from the current fiber. The signal bits and value are
/// returned directly — the VM's dispatch loop stores them in fiber.signal
/// and suspends the fiber.
pub(crate) fn prim_emit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let bits = match resolve_signal_bits(&args[0], "emit", ctx) {
        Ok(bits) => bits,
        Err(err) => return err,
    };

    // Return the signal bits and value directly.
    // The VM's handle_primitive_signal catch-all stores (bits, value)
    // in fiber.signal and returns Some(bits), suspending the fiber.
    (bits, args[1])
}

/// (fiber/status fiber) → keyword
///
/// Returns the fiber's lifecycle status as a keyword.
pub(crate) fn prim_fiber_status(
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
                    format!("fiber/status: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let status = handle.with(|fiber| fiber.status);
    (SIG_OK, status_keyword(status))
}

/// (fiber/value fiber) → value
///
/// Returns the signal payload from the fiber's last signal or return value.
/// Returns nil if the fiber has no signal.
pub(crate) fn prim_fiber_value(
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
                    format!("fiber/value: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let value = handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL));
    (SIG_OK, value)
}

// Declarative primitive definitions for fiber lifecycle operations
primitive! {
    "fiber/new" => prim_fiber_new {
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Create a fiber with a signal mask. Optional :deny withholds capabilities.",
        params: &["closure", "mask"],
        category: "fiber",
        example: "(fiber/new (fn [] 42) |:error| :deny |:io|)",
        aliases: &["fiber"],
        effect: RegionEffect::Fresh,
    }
    "fiber/resume" => prim_fiber_resume {
        signal: (Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_RESUME),
            propagates: 0,
        }),
        arity: Arity::Range(1, 2),
        doc: "Resume a fiber, optionally delivering a value",
        params: &["fiber", "value"],
        category: "fiber",
        example: "(fiber/resume f)",
        aliases: &["resume"],
        effect: RegionEffect::Mixed,
    }
    "fiber/emit" => prim_emit {
        signal: Signal::yields_errors(),
        arity: Arity::Exact(2),
        doc: "Emit a signal from the current fiber",
        params: &["bits", "value"],
        category: "fiber",
        example: "(emit 2 42)",
        aliases: &["emit"],
        effect: RegionEffect::Mixed,
    }
    "fiber/status" => prim_fiber_status {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the fiber's lifecycle status (:new, :alive, :suspended, :dead, :error)",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/status f)",
        effect: RegionEffect::Immediate,
    }
    "fiber/value" => prim_fiber_value {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the signal payload from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/value f)",
        effect: RegionEffect::PassThrough,
    }
    "fiber/set-fuel" => prim_fiber_set_fuel {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Set the instruction budget on a fiber. n is a non-negative integer.",
        params: &["fiber", "n"],
        category: "fiber",
        example: "(fiber/set-fuel f 10000)",
        effect: RegionEffect::Immediate,
    }
    "fiber/fuel" => prim_fiber_fuel {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read remaining fuel. Returns integer or nil if unlimited.",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/fuel f)",
        effect: RegionEffect::Immediate,
    }
    "fiber/clear-fuel" => prim_fiber_clear_fuel {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove the instruction budget, restoring unlimited execution.",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/clear-fuel f)",
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-fibers.lisp
