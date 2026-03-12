//! Coroutine primitives for Elle — implemented as fiber wrappers.
//!
//! Coroutines are fibers with SIG_YIELD mask. All operations delegate
//! to the fiber system.
//!
//! Primitives:
//! - coro/new: Create a fiber with SIG_YIELD mask
//! - coro/resume: Resume via fiber/resume
//! - coro/status: Fiber status with coro/compatible names
//! - coro/done?: Check if fiber is dead or errored
//! - coro/value: Get fiber signal value
//! - coro?: Check if value is a fiber
//! - coro/>iterator: Identity (fibers are iterable)
//! - yield*: Prelude macro for sub-coroutine delegation

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{
    Fiber, FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_RESUME, SIG_YIELD,
};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (coro/new fn) → fiber
///
/// Creates a fiber with SIG_YIELD mask from a closure.
pub(crate) fn prim_make_coroutine(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/new: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(c) = args[0].as_closure() {
        let fiber = Fiber::new((*c).clone(), SIG_YIELD);
        (SIG_OK, Value::fiber(fiber))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("coro/new: expected function, got {}", args[0].type_name()),
            ),
        )
    }
}

/// (coro/status co) → keyword
///
/// Returns the fiber's status as a keyword, mapped to coroutine names:
/// :new → :created, :alive → :running, :dead → :done,
/// :suspended and :error unchanged.
pub(crate) fn prim_coroutine_status(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/status: expected 1 argument, got {}", args.len()),
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
                        "coro/status: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let status = handle.with(|fiber| fiber.status);
    let name = match status {
        FiberStatus::New => "new",
        FiberStatus::Alive => "alive",
        FiberStatus::Paused => "paused",
        FiberStatus::Dead => "dead",
        FiberStatus::Error => "error",
    };

    (SIG_OK, Value::keyword(name))
}

/// (coro/done? co) → bool
pub(crate) fn prim_coroutine_done(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/done?: expected 1 argument, got {}", args.len()),
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
                        "coro/done?: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let status = handle.with(|fiber| fiber.status);
    (
        SIG_OK,
        Value::bool(matches!(status, FiberStatus::Dead | FiberStatus::Error)),
    )
}

/// (coro/value co) → value
///
/// Returns the signal payload from the fiber's last signal.
pub(crate) fn prim_coroutine_value(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/value: expected 1 argument, got {}", args.len()),
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
                        "coro/value: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let value = handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL));
    (SIG_OK, value)
}

/// (coro? val) → bool
///
/// Returns true if the value is a fiber (coroutines are fibers).
pub(crate) fn prim_is_coroutine(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coroutine?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    (SIG_OK, Value::bool(args[0].is_fiber()))
}

/// (coro/resume co) → value
/// (coro/resume co val) → value
///
/// Resume a fiber. Returns SIG_RESUME for the VM to handle.
pub(crate) fn prim_coroutine_resume(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/resume: expected 1-2 arguments, got {}", args.len()),
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
                        "coro/resume: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let resume_value = args.get(1).copied().unwrap_or(Value::NIL);

    // Validate status and store resume value
    let status_err = handle.with_mut(|fiber| match fiber.status {
        FiberStatus::New | FiberStatus::Paused => {
            fiber.signal = Some((SIG_OK, resume_value));
            None
        }
        FiberStatus::Alive => Some(error_val(
            "error",
            "coro/resume: coroutine is already running",
        )),
        FiberStatus::Dead => Some(error_val(
            "error",
            "coro/resume: cannot resume completed coroutine",
        )),
        FiberStatus::Error => Some(error_val(
            "error",
            "coro/resume: cannot resume errored coroutine",
        )),
    });

    if let Some(err) = status_err {
        return (SIG_ERROR, err);
    }

    (SIG_RESUME, args[0])
}

/// (coro/>iterator co) → co
///
/// Identity — fibers are iterable.
pub(crate) fn prim_coroutine_to_iterator(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coro/>iterator: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_fiber() {
        (SIG_OK, args[0])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "coro/>iterator: expected coroutine, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Declarative primitive definitions for coroutine operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "coro/new",
        func: prim_make_coroutine,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Create a coroutine (fiber with SIG_YIELD mask) from a closure",
        params: &["closure"],
        category: "coro",
        example: "(coro/new (fn [] (+ 1 2)))",
        aliases: &["make-coroutine"],
    },
    PrimitiveDef {
        name: "coro/status",
        func: prim_coroutine_status,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Get the status of a coroutine (:new, :alive, :paused, :dead, :error)",
        params: &["coroutine"],
        category: "coro",
        example: "(coro/status co)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro/done?",
        func: prim_coroutine_done,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Check if a coroutine is done (dead or errored)",
        params: &["coroutine"],
        category: "coro",
        example: "(coro/done? co)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro/value",
        func: prim_coroutine_value,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Get the signal payload from a coroutine's last signal",
        params: &["coroutine"],
        category: "coro",
        example: "(coro/value co)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro/resume",
        func: prim_coroutine_resume,
        effect: Signal::yields_errors(),
        arity: Arity::Range(1, 2),
        doc: "Resume a coroutine, optionally delivering a value",
        params: &["coroutine", "value"],
        category: "coro",
        example: "(coro/resume co)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro/>iterator",
        func: prim_coroutine_to_iterator,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Convert a coroutine to an iterator (identity — fibers are iterable)",
        params: &["coroutine"],
        category: "coro",
        example: "(coro/>iterator co)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use crate::value::{Arity, Closure};
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
            template: Rc::new(crate::value::ClosureTemplate {
                bytecode: Rc::new(bytecode),
                arity: Arity::Exact(0),
                num_locals: 0,
                num_captures: 0,
                num_params: 0,
                constants: Rc::new(vec![Value::NIL]),
                signal: Signal::inert(),
                lbox_params_mask: 0,
                lbox_locals_mask: 0,
                symbol_names: Rc::new(std::collections::HashMap::new()),
                location_map: Rc::new(crate::error::LocationMap::new()),
                jit_code: None,
                lir_function: None,
                doc: None,
                syntax: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
            }),
            env: Rc::new(vec![]),
        })
    }

    #[test]
    fn test_make_coroutine_creates_fiber() {
        let closure = make_test_closure();
        let (sig, result_val) = prim_make_coroutine(&[closure]);
        assert_eq!(sig, SIG_OK);
        assert!(result_val.is_fiber(), "coro/new should create a fiber");
        let handle = result_val.as_fiber().unwrap();
        handle.with(|fiber| {
            assert_eq!(fiber.status, FiberStatus::New);
            assert_eq!(fiber.mask, SIG_YIELD);
        });
    }

    #[test]
    fn test_make_coroutine_wrong_type() {
        let (sig, _) = prim_make_coroutine(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_coroutine_done_false_when_new() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        let (sig, done) = prim_coroutine_done(&[co]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(done, Value::bool(false));
    }

    #[test]
    fn test_coroutine_done_true_when_dead() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        co.as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Dead);
        let (sig, done) = prim_coroutine_done(&[co]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(done, Value::bool(true));
    }

    #[test]
    fn test_is_coroutine_true_for_fiber() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        let (sig, result) = prim_is_coroutine(&[co]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_is_coroutine_false_for_non_fiber() {
        let (sig, result) = prim_is_coroutine(&[Value::int(42)]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_coroutine_value_nil_when_no_signal() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        let (sig, value) = prim_coroutine_value(&[co]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(value, Value::NIL);
    }

    #[test]
    fn test_coroutine_resume_returns_sig_resume() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        let (sig, val) = prim_coroutine_resume(&[co]);
        assert_eq!(sig, SIG_RESUME);
        assert!(val.is_fiber());
    }

    #[test]
    fn test_coroutine_resume_dead_returns_error() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        co.as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Dead);
        let (sig, _) = prim_coroutine_resume(&[co]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_coroutine_resume_wrong_type() {
        let (sig, _) = prim_coroutine_resume(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_coroutine_to_iterator_identity() {
        let closure = make_test_closure();
        let (_, co) = prim_make_coroutine(&[closure]);
        let (sig, iter) = prim_coroutine_to_iterator(std::slice::from_ref(&co));
        assert_eq!(sig, SIG_OK);
        assert!(iter.is_fiber());
    }
}
