//! Coroutine primitives for Elle — implemented as fiber wrappers.
//!
//! Coroutines are fibers with SIG_YIELD mask. The user-facing API is
//! preserved for backward compatibility, but all operations delegate
//! to the fiber system.
//!
//! Primitives:
//! - make-coroutine: Create a fiber with SIG_YIELD mask
//! - coroutine-resume: Resume via fiber/resume
//! - coroutine-status: Fiber status with coroutine-compatible names
//! - coroutine-done?: Check if fiber is dead or errored
//! - coroutine-value: Get fiber signal value
//! - coroutine?: Check if value is a fiber
//! - coroutine->iterator: Identity (fibers are iterable)
//! - yield-from: Stub (not yet supported)

use crate::value::fiber::{
    Fiber, FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_RESUME, SIG_YIELD,
};
use crate::value::{error_val, Value};

/// (make-coroutine fn) → fiber
///
/// Creates a fiber with SIG_YIELD mask from a closure.
pub fn prim_make_coroutine(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("make-coroutine: expected 1 argument, got {}", args.len()),
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
                format!(
                    "make-coroutine: expected function, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// (coroutine-status co) → keyword
///
/// Returns the fiber's status as a keyword, mapped to coroutine names:
/// :new → :created, :alive → :running, :dead → :done,
/// :suspended and :error unchanged.
pub fn prim_coroutine_status(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coroutine-status: expected 1 argument, got {}", args.len()),
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
                        "coroutine-status: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let status = handle.with(|fiber| fiber.status);
    let name = match status {
        FiberStatus::New => "created",
        FiberStatus::Alive => "running",
        FiberStatus::Suspended => "suspended",
        FiberStatus::Dead => "done",
        FiberStatus::Error => "error",
    };

    // Intern as keyword via thread-local symbol table
    unsafe {
        if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
            let id = (*symbols_ptr).intern(name);
            (SIG_OK, Value::keyword(id.0))
        } else {
            (SIG_OK, Value::string(name))
        }
    }
}

/// (coroutine-done? co) → bool
pub fn prim_coroutine_done(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coroutine-done?: expected 1 argument, got {}", args.len()),
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
                        "coroutine-done?: expected coroutine, got {}",
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

/// (coroutine-value co) → value
///
/// Returns the signal payload from the fiber's last signal.
pub fn prim_coroutine_value(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("coroutine-value: expected 1 argument, got {}", args.len()),
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
                        "coroutine-value: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let value = handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL));
    (SIG_OK, value)
}

/// (coroutine? val) → bool
///
/// Returns true if the value is a fiber (coroutines are fibers).
pub fn prim_is_coroutine(args: &[Value]) -> (SignalBits, Value) {
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

/// (coroutine-resume co) → value
/// (coroutine-resume co val) → value
///
/// Resume a fiber. Returns SIG_RESUME for the VM to handle.
pub fn prim_coroutine_resume(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "coroutine-resume: expected 1-2 arguments, got {}",
                    args.len()
                ),
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
                        "coroutine-resume: expected coroutine, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let resume_value = args.get(1).copied().unwrap_or(Value::NIL);

    // Validate status and store resume value
    let status_err = handle.with_mut(|fiber| match fiber.status {
        FiberStatus::New | FiberStatus::Suspended => {
            fiber.signal = Some((SIG_OK, resume_value));
            None
        }
        FiberStatus::Alive => Some(error_val(
            "error",
            "coroutine-resume: coroutine is already running",
        )),
        FiberStatus::Dead => Some(error_val(
            "error",
            "coroutine-resume: cannot resume completed coroutine",
        )),
        FiberStatus::Error => Some(error_val(
            "error",
            "coroutine-resume: cannot resume errored coroutine",
        )),
    });

    if let Some(err) = status_err {
        return (SIG_ERROR, err);
    }

    (SIG_RESUME, args[0])
}

/// (yield-from co) → error
///
/// Dropped in the fiber migration. See issue #294 for yield* design.
pub fn prim_yield_from(args: &[Value]) -> (SignalBits, Value) {
    let _ = args;
    (
        SIG_ERROR,
        error_val(
            "error",
            "yield-from: not yet supported with fibers (see issue #294 for yield*)",
        ),
    )
}

/// (coroutine->iterator co) → co
///
/// Identity — fibers are iterable.
pub fn prim_coroutine_to_iterator(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "coroutine->iterator: expected 1 argument, got {}",
                    args.len()
                ),
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
                    "coroutine->iterator: expected coroutine, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
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
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![Value::NIL]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
            jit_code: None,
            lir_function: None,
        })
    }

    #[test]
    fn test_make_coroutine_creates_fiber() {
        let closure = make_test_closure();
        let (sig, result_val) = prim_make_coroutine(&[closure]);
        assert_eq!(sig, SIG_OK);
        assert!(
            result_val.is_fiber(),
            "make-coroutine should create a fiber"
        );
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
    fn test_yield_from_returns_error() {
        let (sig, _) = prim_yield_from(&[Value::int(42)]);
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
