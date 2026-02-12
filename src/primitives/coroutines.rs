//! Coroutine primitives for Elle
//!
//! Provides the user-facing API for colorless coroutines:
//! - make-coroutine: Create a coroutine from a function
//! - coroutine-resume: Resume a suspended coroutine
//! - coroutine-status: Get the status of a coroutine
//! - coroutine-value: Get the last yielded value
//! - yield-from: Delegate to a sub-coroutine
//! - coroutine->iterator: Convert coroutine to iterator
//! - coroutine-next: Get next value from coroutine iterator

use crate::value::{Coroutine, CoroutineState, Value};
use crate::vm::VM;
use std::rc::Rc;

/// F1: Create a coroutine from a function
///
/// (make-coroutine fn) -> coroutine
pub fn prim_make_coroutine(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "make-coroutine requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Closure(c) => {
            let coroutine = Coroutine {
                closure: c.clone(),
                state: CoroutineState::Created,
                yielded_value: None,
            };
            Ok(Value::Coroutine(Rc::new(coroutine)))
        }
        Value::JitClosure(jc) => {
            if let Some(source) = &jc.source {
                let coroutine = Coroutine {
                    closure: source.clone(),
                    state: CoroutineState::Created,
                    yielded_value: None,
                };
                Ok(Value::Coroutine(Rc::new(coroutine)))
            } else {
                Err("JitClosure has no source for coroutine".to_string())
            }
        }
        other => Err(format!(
            "make-coroutine requires a function, got {}",
            other.type_name()
        )),
    }
}

/// F3: Get the status of a coroutine
///
/// (coroutine-status co) -> string
pub fn prim_coroutine_status(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine-status requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => {
            let status = match &co.state {
                CoroutineState::Created => "created",
                CoroutineState::Running => "running",
                CoroutineState::Suspended => "suspended",
                CoroutineState::Done => "done",
                CoroutineState::Error(_) => "error",
            };
            Ok(Value::String(status.to_string().into()))
        }
        other => Err(format!(
            "coroutine-status requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// Check if a coroutine is done
///
/// (coroutine-done? co) -> bool
pub fn prim_coroutine_done(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine-done? requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => Ok(Value::Bool(matches!(co.state, CoroutineState::Done))),
        other => Err(format!(
            "coroutine-done? requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// Get the last yielded value from a coroutine
///
/// (coroutine-value co) -> value
pub fn prim_coroutine_value(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine-value requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => Ok(co.yielded_value.clone().unwrap_or(Value::Nil)),
        other => Err(format!(
            "coroutine-value requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// Check if a value is a coroutine
///
/// (coroutine? val) -> bool
pub fn prim_is_coroutine(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine? requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    Ok(Value::Bool(matches!(args[0], Value::Coroutine(_))))
}

/// F2/F4: Resume a coroutine
///
/// (coroutine-resume co) -> value
/// (coroutine-resume co val) -> value
///
/// Resumes execution of a suspended coroutine.
/// If the coroutine yields, returns the yielded value.
/// If the coroutine completes, returns the final value.
pub fn prim_coroutine_resume(args: &[Value], vm: &mut VM) -> Result<Value, String> {
    if args.is_empty() || args.len() > 2 {
        return Err(format!(
            "coroutine-resume requires 1 or 2 arguments, got {}",
            args.len()
        ));
    }

    let _resume_value = args.get(1).cloned().unwrap_or(Value::Nil);

    match &args[0] {
        Value::Coroutine(co) => {
            // We need to mutate the coroutine state
            // Clone and update
            let mut new_co = (**co).clone();

            match &new_co.state {
                CoroutineState::Created => {
                    // First resume - start execution
                    new_co.state = CoroutineState::Running;

                    // Execute the closure
                    let result = vm.execute_bytecode(
                        &new_co.closure.bytecode,
                        &new_co.closure.constants,
                        Some(&new_co.closure.env),
                    );

                    match result {
                        Ok(value) => {
                            new_co.state = CoroutineState::Done;
                            new_co.yielded_value = Some(value.clone());
                            Ok(value)
                        }
                        Err(e) => {
                            new_co.state = CoroutineState::Error(e.clone());
                            Err(e)
                        }
                    }
                }
                CoroutineState::Suspended => {
                    // Resume from suspension - not yet fully implemented
                    // For now, return an error
                    Err("Resuming suspended coroutines not yet implemented".to_string())
                }
                CoroutineState::Running => Err("Coroutine is already running".to_string()),
                CoroutineState::Done => Err("Cannot resume completed coroutine".to_string()),
                CoroutineState::Error(e) => Err(format!("Cannot resume errored coroutine: {}", e)),
            }
        }
        other => Err(format!(
            "coroutine-resume requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// F5: Delegate to a sub-coroutine
///
/// (yield-from co) -> value
///
/// Yields all values from the sub-coroutine until it completes,
/// then returns the sub-coroutine's final value.
pub fn prim_yield_from(args: &[Value], vm: &mut VM) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "yield-from requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => {
            // For now, just run the coroutine to completion
            // Full yield-from requires CPS integration
            let current_co = (**co).clone();

            loop {
                match &current_co.state {
                    CoroutineState::Created | CoroutineState::Suspended => {
                        // Resume the coroutine
                        let result = prim_coroutine_resume(
                            &[Value::Coroutine(Rc::new(current_co.clone()))],
                            vm,
                        )?;

                        // Update state based on result
                        if matches!(current_co.state, CoroutineState::Done) {
                            return Ok(result);
                        }
                        // If suspended, we should yield the value up
                        // For now, just continue
                    }
                    CoroutineState::Done => {
                        return Ok(current_co.yielded_value.clone().unwrap_or(Value::Nil));
                    }
                    CoroutineState::Error(e) => {
                        return Err(e.clone());
                    }
                    CoroutineState::Running => {
                        return Err("Sub-coroutine is already running".to_string());
                    }
                }
            }
        }
        other => Err(format!(
            "yield-from requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// F6: Get an iterator from a coroutine
///
/// (coroutine->iterator co) -> iterator
///
/// Creates an iterator that yields values from the coroutine.
pub fn prim_coroutine_to_iterator(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine->iterator requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(_) => {
            // For now, just return the coroutine itself
            // The for loop implementation will need to recognize coroutines
            Ok(args[0].clone())
        }
        other => Err(format!(
            "coroutine->iterator requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

/// Get the next value from a coroutine iterator
///
/// (coroutine-next co) -> (value . done?)
///
/// Returns a pair of (value, done-flag).
pub fn prim_coroutine_next(args: &[Value], vm: &mut VM) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "coroutine-next requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => {
            if matches!(co.state, CoroutineState::Done) {
                // Return (nil . #t) to indicate done
                Ok(crate::value::cons(Value::Nil, Value::Bool(true)))
            } else {
                // Resume and get next value
                let result = prim_coroutine_resume(args, vm)?;

                // Check if done after resume
                // For now, assume not done unless we got an error
                Ok(crate::value::cons(result, Value::Bool(false)))
            }
        }
        other => Err(format!(
            "coroutine-next requires a coroutine, got {}",
            other.type_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::effects::Effect;
    use crate::value::{Arity, Closure};
    use crate::vm::VM;

    fn make_test_closure() -> Value {
        // Create a closure with bytecode that returns nil
        // Bytecode: LoadConst(0), Return
        // Constants: [nil]
        use crate::compiler::bytecode::Instruction;
        let bytecode = vec![
            Instruction::LoadConst as u8,
            0, // Load constant 0 (nil) - high byte
            0, // Load constant 0 (nil) - low byte
            Instruction::Return as u8,
        ];

        Value::Closure(Rc::new(Closure {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![Value::Nil]),
            source_ast: None,
            effect: Effect::Pure,
        }))
    }

    #[test]
    fn test_make_coroutine() {
        let closure = make_test_closure();
        let result = prim_make_coroutine(&[closure]);
        assert!(result.is_ok());

        if let Value::Coroutine(co) = result.unwrap() {
            assert!(matches!(co.state, CoroutineState::Created));
        } else {
            panic!("Expected coroutine");
        }
    }

    #[test]
    fn test_make_coroutine_wrong_type() {
        let result = prim_make_coroutine(&[Value::Int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_coroutine_status() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let status = prim_coroutine_status(&[co]).unwrap();
        assert_eq!(status, Value::String("created".to_string().into()));
    }

    #[test]
    fn test_coroutine_done() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let done = prim_coroutine_done(&[co]).unwrap();
        assert_eq!(done, Value::Bool(false));
    }

    #[test]
    fn test_is_coroutine() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();

        assert_eq!(prim_is_coroutine(&[co]).unwrap(), Value::Bool(true));
        assert_eq!(
            prim_is_coroutine(&[Value::Int(42)]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_coroutine_value() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let value = prim_coroutine_value(&[co]).unwrap();
        assert_eq!(value, Value::Nil);
    }

    #[test]
    fn test_coroutine_resume_wrong_type() {
        let mut vm = VM::new();
        let result = prim_coroutine_resume(&[Value::Int(42)], &mut vm);
        assert!(result.is_err());
    }

    #[test]
    fn test_coroutine_resume_created() {
        let mut vm = VM::new();
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let result = prim_coroutine_resume(&[co], &mut vm);
        // Should succeed with empty bytecode returning nil
        if let Err(e) = &result {
            eprintln!("Error: {}", e);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_yield_from_wrong_type() {
        let mut vm = VM::new();
        let result = prim_yield_from(&[Value::Int(42)], &mut vm);
        assert!(result.is_err());
    }

    #[test]
    fn test_coroutine_to_iterator() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let iter = prim_coroutine_to_iterator(std::slice::from_ref(&co)).unwrap();
        assert!(matches!(iter, Value::Coroutine(_)));
    }

    #[test]
    fn test_coroutine_next_wrong_type() {
        let mut vm = VM::new();
        let result = prim_coroutine_next(&[Value::Int(42)], &mut vm);
        assert!(result.is_err());
    }

    #[test]
    fn test_coroutine_next_done() {
        let mut vm = VM::new();
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        // Get next should return a cons pair
        let result = prim_coroutine_next(&[co], &mut vm);
        assert!(result.is_ok());
        // Result should be a cons pair (value . done?)
        if let Value::Cons(cons) = result.unwrap() {
            // The first element should be the value (nil in this case)
            assert_eq!(cons.first, Value::Nil);
            // The second element should be a boolean (done flag)
            assert!(matches!(cons.rest, Value::Bool(_)));
        } else {
            panic!("Expected cons pair");
        }
    }
}
