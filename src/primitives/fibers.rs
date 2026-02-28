//! Fiber primitives for Elle.
//!
//! Fibers are independent execution contexts with their own stack, frames,
//! and signal state. They communicate via signals — a fiber can emit a
//! signal, and its parent can catch or propagate it based on the mask.
//!
//! Primitives:
//! - fiber/new: Create a fiber from a closure with a signal mask
//! - fiber/resume: Resume a suspended fiber, delivering a value
//! - fiber/signal: Emit a signal from the current fiber
//! - fiber/status: Get fiber lifecycle status (:new, :alive, :suspended, :dead, :error)
//! - fiber/value: Get signal payload from last signal
//! - fiber/bits: Get signal bits from last signal
//! - fiber/mask: Get the fiber's signal mask
//! - fiber/parent: Get parent fiber or nil
//! - fiber/child: Get most recently resumed child fiber or nil
//! - fiber/propagate: Re-raise caught signal preserving child chain
//! - fiber/cancel: Inject error into suspended fiber
//! - fiber?: Type predicate

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{
    Fiber, FiberStatus, SignalBits, SIG_CANCEL, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_RESUME,
};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Return a keyword Value for a FiberStatus.
fn status_keyword(status: FiberStatus) -> Value {
    Value::keyword(status.as_str())
}

/// (fiber/new fn mask) → fiber
///
/// Create a fiber from a closure and a signal mask. The mask determines
/// which signals the parent catches when resuming this fiber.
pub fn prim_fiber_new(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/new: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let closure = match args[0].as_closure() {
        Some(c) => c.clone(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/new: expected closure, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let mask = match args[1].as_int() {
        Some(m) => m as SignalBits,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "fiber/new: expected integer mask, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    let fiber = Fiber::new(closure, mask);
    (SIG_OK, Value::fiber(fiber))
}

/// (fiber/resume fiber) → value
/// (fiber/resume fiber value) → value
///
/// Resume a fiber. If the fiber is New, starts execution. If Suspended,
/// delivers the value and continues from where it left off.
///
/// Returns SIG_RESUME — the VM handles the actual fiber swap.
pub fn prim_fiber_resume(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
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
                error_val(
                    "type-error",
                    format!("fiber/resume: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let resume_value = args.get(1).copied().unwrap_or(Value::NIL);

    // Validate fiber status and store resume value
    let status_err = handle.with_mut(|fiber| match fiber.status {
        FiberStatus::New | FiberStatus::Suspended => {
            fiber.signal = Some((SIG_OK, resume_value));
            None
        }
        FiberStatus::Alive => Some(error_val("error", "fiber/resume: fiber is already running")),
        FiberStatus::Dead => Some(error_val(
            "error",
            "fiber/resume: cannot resume completed fiber",
        )),
        FiberStatus::Error => Some(error_val(
            "error",
            "fiber/resume: cannot resume errored fiber",
        )),
    });

    if let Some(err) = status_err {
        return (SIG_ERROR, err);
    }

    // Return SIG_RESUME — VM will handle the fiber swap
    (SIG_RESUME, args[0])
}

/// (fiber/signal bits value) → suspends
///
/// Emit a signal from the current fiber. The signal bits and value are
/// returned directly — the VM's dispatch loop stores them in fiber.signal
/// and suspends the fiber.
pub fn prim_fiber_signal(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/signal: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let bits = match args[0].as_int() {
        Some(b) => b as SignalBits,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "fiber/signal: expected integer bits, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    // Return the signal bits and value directly.
    // The VM's handle_primitive_signal catch-all stores (bits, value)
    // in fiber.signal and returns Some(bits), suspending the fiber.
    (bits, args[1])
}

/// (fiber/status fiber) → keyword
///
/// Returns the fiber's lifecycle status as a keyword.
pub fn prim_fiber_status(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/status: expected 1 argument, got {}", args.len()),
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
pub fn prim_fiber_value(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/value: expected 1 argument, got {}", args.len()),
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
                    format!("fiber/value: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let value = handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL));
    (SIG_OK, value)
}

/// (fiber/bits fiber) → int
///
/// Returns the signal bits from the fiber's last signal.
/// Returns 0 if the fiber has no signal.
pub fn prim_fiber_bits(args: &[Value]) -> (SignalBits, Value) {
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

    let bits = handle.with(|fiber| fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(0));
    (SIG_OK, Value::int(bits as i64))
}

/// (fiber/mask fiber) → int
///
/// Returns the fiber's signal mask.
pub fn prim_fiber_mask(args: &[Value]) -> (SignalBits, Value) {
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
    (SIG_OK, Value::int(mask as i64))
}

/// (fiber? value) → bool
///
/// Type predicate: returns true if the value is a fiber.
pub fn prim_is_fiber(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_fiber_parent(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_fiber_child(args: &[Value]) -> (SignalBits, Value) {
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
/// Re-raise a caught signal from a child fiber, preserving the child chain
/// for stack traces. The fiber must be in :error or :suspended status.
///
/// Returns SIG_PROPAGATE — the VM sets parent.child = fiber and propagates
/// the fiber's signal upward.
pub fn prim_fiber_propagate(args: &[Value]) -> (SignalBits, Value) {
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

    // Validate: fiber must be in error or suspended state with a signal
    let has_signal = handle.with(|fiber| {
        matches!(fiber.status, FiberStatus::Error | FiberStatus::Suspended)
            && fiber.signal.is_some()
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

/// (fiber/cancel fiber value) → value
///
/// Inject an error into a suspended fiber. The error is injected directly
/// into the target fiber (does not walk the child chain).
///
/// Returns SIG_CANCEL — the VM handles the cancellation.
pub fn prim_fiber_cancel(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/cancel: expected 2 arguments, got {}", args.len()),
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
        FiberStatus::New | FiberStatus::Suspended => {
            // Valid for cancel — store the error value on the fiber
            handle.with_mut(|fiber| {
                fiber.signal = Some((SIG_ERROR, args[1]));
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

/// Declarative primitive definitions for fiber operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fiber/new",
        func: prim_fiber_new,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Create a fiber from a closure with a signal mask",
        params: &["closure", "mask"],
        category: "fiber",
        example: "(fiber/new (fn [] 42) 0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/resume",
        func: prim_fiber_resume,
        effect: Effect::yields_raises(),
        arity: Arity::Range(1, 2),
        doc: "Resume a fiber, optionally delivering a value",
        params: &["fiber", "value"],
        category: "fiber",
        example: "(fiber/resume f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/signal",
        func: prim_fiber_signal,
        effect: Effect::yields_raises(),
        arity: Arity::Exact(2),
        doc: "Emit a signal from the current fiber",
        params: &["bits", "value"],
        category: "fiber",
        example: "(fiber/signal 2 42)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/status",
        func: prim_fiber_status,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get the fiber's lifecycle status (:new, :alive, :suspended, :dead, :error)",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/status f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/value",
        func: prim_fiber_value,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get the signal payload from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/value f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/bits",
        func: prim_fiber_bits,
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::yields_raises(),
        arity: Arity::Exact(1),
        doc: "Re-raise a caught signal from a child fiber, preserving the child chain",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/propagate f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/cancel",
        func: prim_fiber_cancel,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Inject an error into a suspended fiber",
        params: &["fiber", "error"],
        category: "fiber",
        example: "(fiber/cancel f error)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::error::LocationMap;
    use crate::value::fiber::{SIG_ERROR as FIBER_SIG_ERROR, SIG_YIELD};
    use crate::value::{Arity, Closure};
    use std::collections::HashMap;
    use std::rc::Rc;

    fn make_test_closure() -> Value {
        use crate::compiler::bytecode::Instruction;
        let bytecode = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::Return as u8,
        ];

        Value::closure(Closure {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![Value::int(42)]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
        })
    }

    #[test]
    fn test_fiber_new() {
        let closure = make_test_closure();
        let mask = Value::int((SIG_ERROR | SIG_YIELD) as i64);
        let (sig, fiber_val) = prim_fiber_new(&[closure, mask]);
        assert_eq!(sig, SIG_OK);
        assert!(fiber_val.is_fiber());

        let handle = fiber_val.as_fiber().unwrap();
        handle.with(|fiber| {
            assert_eq!(fiber.status, FiberStatus::New);
            assert_eq!(fiber.mask, FIBER_SIG_ERROR | SIG_YIELD);
        });
    }

    #[test]
    fn test_fiber_new_wrong_type() {
        let (sig, _) = prim_fiber_new(&[Value::int(42), Value::int(0)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_new_wrong_arity() {
        let (sig, _) = prim_fiber_new(&[make_test_closure()]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_returns_sig_resume() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, val) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_RESUME);
        assert!(val.is_fiber());
    }

    #[test]
    fn test_fiber_resume_dead_fiber() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        // Manually set to Dead
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Dead);
        let (sig, _) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_alive_fiber() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Alive);
        let (sig, _) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_with_value() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_resume(&[fiber_val, Value::int(99)]);
        assert_eq!(sig, SIG_RESUME);
        // Check that the resume value was stored
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.signal, Some((SIG_OK, Value::int(99))));
        });
    }

    #[test]
    fn test_fiber_signal() {
        let bits = Value::int(SIG_YIELD as i64);
        let value = Value::int(42);
        let (sig, val) = prim_fiber_signal(&[bits, value]);
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_fiber_signal_wrong_arity() {
        let (sig, _) = prim_fiber_signal(&[Value::int(0)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_status() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, status) = prim_fiber_status(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert!(status.is_keyword(), "Expected keyword, got {:?}", status);
    }

    #[test]
    fn test_fiber_status_transitions() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        // All statuses should return keywords
        for status in [
            FiberStatus::New,
            FiberStatus::Alive,
            FiberStatus::Suspended,
            FiberStatus::Dead,
            FiberStatus::Error,
        ] {
            fiber_val
                .as_fiber()
                .unwrap()
                .with_mut(|f| f.status = status);
            let (sig, val) = prim_fiber_status(&[fiber_val]);
            assert_eq!(sig, SIG_OK);
            assert!(
                val.is_keyword(),
                "Expected keyword for {:?}, got {:?}",
                status,
                val
            );
        }
    }

    #[test]
    fn test_fiber_value() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        // No signal yet — returns nil
        let (sig, val) = prim_fiber_value(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::NIL);

        // Set a signal
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.signal = Some((SIG_YIELD, Value::int(42))));
        let (sig, val) = prim_fiber_value(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_fiber_bits() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        // No signal yet — returns 0
        let (sig, val) = prim_fiber_bits(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(0));

        // Set a signal
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.signal = Some((SIG_YIELD, Value::int(42))));
        let (sig, val) = prim_fiber_bits(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(SIG_YIELD as i64));
    }

    #[test]
    fn test_fiber_mask() {
        let closure = make_test_closure();
        let mask = (FIBER_SIG_ERROR | SIG_YIELD) as i64;
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(mask)]);
        let (sig, val) = prim_fiber_mask(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(mask));
    }

    #[test]
    fn test_is_fiber() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        let (sig, val) = prim_is_fiber(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::bool(true));

        let (sig, val) = prim_is_fiber(&[Value::int(42)]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::bool(false));
    }

    #[test]
    fn test_fiber_resume_wrong_type() {
        let (sig, _) = prim_fiber_resume(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_status_wrong_type() {
        let (sig, _) = prim_fiber_status(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_value_wrong_type() {
        let (sig, _) = prim_fiber_value(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_bits_wrong_type() {
        let (sig, _) = prim_fiber_bits(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_mask_wrong_type() {
        let (sig, _) = prim_fiber_mask(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }
}
