//! Fiber lifecycle primitives.
//!
//! Core fiber operations: creation, resumption, signaling, status, and
//! value extraction. Introspection and management primitives (bits, mask,
//! parent, child, propagate, cancel, fiber?) are in `fiber_introspect.rs`.

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{Fiber, FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_RESUME};
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
pub(crate) fn prim_fiber_new(args: &[Value]) -> (SignalBits, Value) {
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
        Some(m) => SignalBits::new(m as u32),
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
pub(crate) fn prim_fiber_resume(args: &[Value]) -> (SignalBits, Value) {
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
        FiberStatus::New | FiberStatus::Paused => {
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
pub(crate) fn prim_fiber_signal(args: &[Value]) -> (SignalBits, Value) {
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
        Some(b) => SignalBits::new(b as u32),
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
pub(crate) fn prim_fiber_status(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_fiber_value(args: &[Value]) -> (SignalBits, Value) {
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

/// Declarative primitive definitions for fiber lifecycle operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fiber/new",
        func: prim_fiber_new,
        effect: Effect::inert(),
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
        effect: Effect::yields_errors(),
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
        effect: Effect::yields_errors(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Get the signal payload from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/value f)",
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
        use crate::value::ClosureTemplate;
        let bytecode = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::Return as u8,
        ];

        let template = Rc::new(ClosureTemplate {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants: Rc::new(vec![Value::int(42)]),
            effect: Effect::inert(),
            cell_params_mask: 0,
            cell_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });

        Value::closure(Closure {
            template,
            env: Rc::new(vec![]),
        })
    }

    #[test]
    fn test_fiber_new() {
        let closure = make_test_closure();
        let mask = Value::int((SIG_ERROR | SIG_YIELD).bits() as i64);
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
        let bits = Value::int(SIG_YIELD.bits() as i64);
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
            FiberStatus::Paused,
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
}
