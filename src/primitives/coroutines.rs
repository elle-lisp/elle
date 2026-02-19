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

use crate::error::LResult;
use crate::value::{Condition, Coroutine, CoroutineState, Value};
use crate::vm::{VmResult, VM};

/// F1: Create a coroutine from a function
///
/// (make-coroutine fn) -> coroutine
pub fn prim_make_coroutine(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "make-coroutine: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(c) = args[0].as_closure() {
        if c.effect.is_pure() {
            eprintln!("warning: make-coroutine: closure has Pure effect and will never yield");
        }
        let coroutine = Coroutine::new((*c).clone());
        Ok(Value::coroutine(coroutine))
    } else {
        Err(Condition::type_error(format!(
            "make-coroutine: expected function, got {}",
            args[0].type_name()
        )))
    }
}

/// F3: Get the status of a coroutine
///
/// (coroutine-status co) -> string
pub fn prim_coroutine_status(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coroutine-status: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(co) = args[0].as_coroutine() {
        let borrowed = co.borrow();
        let status = match &borrowed.state {
            CoroutineState::Created => "created",
            CoroutineState::Running => "running",
            CoroutineState::Suspended => "suspended",
            CoroutineState::Done => "done",
            CoroutineState::Error(_) => "error",
        };
        Ok(Value::string(status))
    } else {
        Err(Condition::type_error(format!(
            "coroutine-status: expected coroutine, got {}",
            args[0].type_name()
        )))
    }
}

/// Check if a coroutine is done
///
/// (coroutine-done? co) -> bool
pub fn prim_coroutine_done(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coroutine-done?: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(co) = args[0].as_coroutine() {
        let borrowed = co.borrow();
        Ok(Value::bool(matches!(
            borrowed.state,
            CoroutineState::Done | CoroutineState::Error(_)
        )))
    } else {
        Err(Condition::type_error(format!(
            "coroutine-done?: expected coroutine, got {}",
            args[0].type_name()
        )))
    }
}

/// Get the last yielded value from a coroutine
///
/// (coroutine-value co) -> value
pub fn prim_coroutine_value(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coroutine-value: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(co) = args[0].as_coroutine() {
        let borrowed = co.borrow();
        // yielded_value is now Option<crate::value::Value> directly
        Ok(borrowed.yielded_value.unwrap_or(Value::NIL))
    } else {
        Err(Condition::type_error(format!(
            "coroutine-value: expected coroutine, got {}",
            args[0].type_name()
        )))
    }
}

/// Check if a value is a coroutine
///
/// (coroutine? val) -> bool
pub fn prim_is_coroutine(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coroutine?: expected 1 argument, got {}",
            args.len()
        )));
    }

    Ok(Value::bool(args[0].is_coroutine()))
}

/// F2/F4: Resume a coroutine
///
/// (coroutine-resume co) -> value
/// (coroutine-resume co val) -> value
///
/// Resumes execution of a suspended coroutine.
/// If the coroutine yields, returns the yielded value.
/// If the coroutine completes, returns the final value.
pub fn prim_coroutine_resume(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.is_empty() || args.len() > 2 {
        let cond = Condition::arity_error(format!(
            "coroutine-resume: expected 1-2 arguments, got {}",
            args.len()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    let resume_value = args.get(1).cloned().unwrap_or(Value::EMPTY_LIST);

    if let Some(co) = args[0].as_coroutine() {
        // Check for yield-from delegation
        // If this coroutine has a delegate, forward the resume to it
        {
            let borrowed = co.borrow();
            if let Some(delegate_val) = borrowed.delegate {
                drop(borrowed); // Release borrow before recursive call

                // Resume the delegate
                let delegate_result = prim_coroutine_resume(&[delegate_val, resume_value], vm)?;

                // Check delegate state
                if let Some(delegate_co) = delegate_val.as_coroutine() {
                    let delegate_state = {
                        let delegate_borrowed = delegate_co.borrow();
                        delegate_borrowed.state.clone()
                    };

                    match delegate_state {
                        CoroutineState::Done => {
                            // Delegate completed - clear delegation and complete outer
                            let mut outer_borrowed = co.borrow_mut();
                            outer_borrowed.delegate = None;
                            outer_borrowed.state = CoroutineState::Done;
                            outer_borrowed.yielded_value = Some(delegate_result);
                            return Ok(delegate_result);
                        }
                        CoroutineState::Suspended => {
                            // Delegate yielded - outer stays suspended, return yielded value
                            let mut outer_borrowed = co.borrow_mut();
                            outer_borrowed.yielded_value = Some(delegate_result);
                            return Ok(delegate_result);
                        }
                        CoroutineState::Error(e) => {
                            // Delegate errored - propagate error
                            let mut outer_borrowed = co.borrow_mut();
                            outer_borrowed.delegate = None;
                            outer_borrowed.state = CoroutineState::Error(e.clone());
                            let cond = Condition::error(format!(
                                "yield-from: delegate coroutine errored: {}",
                                e
                            ));
                            vm.current_exception = Some(std::rc::Rc::new(cond));
                            return Ok(Value::NIL);
                        }
                        _ => {
                            // Unexpected state - just return the result
                            return Ok(delegate_result);
                        }
                    }
                }
                return Ok(delegate_result);
            }
        }

        // Borrow mutably to update the coroutine state
        let mut borrowed = co.borrow_mut();

        match &borrowed.state {
            CoroutineState::Created => {
                // Warn on first resume if closure has Pure effect
                if borrowed.closure.effect.is_pure() {
                    eprintln!("warning: coroutine-resume: coroutine's closure has Pure effect; it will complete without yielding");
                }
                // First resume - start execution using bytecode path
                borrowed.state = CoroutineState::Running;

                // Get closure info before releasing borrow
                let bytecode = borrowed.closure.bytecode.clone();
                let constants = borrowed.closure.constants.clone();
                let closure_env = borrowed.closure.env.clone();
                let num_locals = borrowed.closure.num_locals;
                let num_captures = borrowed.closure.num_captures;

                // Release the borrow before calling vm.execute_bytecode_coroutine
                drop(borrowed);

                // Set up the environment for the coroutine
                // The closure environment contains: [captures..., parameters..., locals...]
                // Since a coroutine is called with no arguments, we need to allocate space for locals
                let mut env = (*closure_env).clone();

                // Calculate number of locally-defined variables
                // num_locals = params.len() + captures.len() + locals.len()
                // Since a coroutine has no parameters, we need to allocate space for all locals
                let num_locally_defined = num_locals.saturating_sub(num_captures);

                // Add empty LocalCells for locally-defined variables
                for _ in env.len()..num_captures + num_locally_defined {
                    let empty_cell = Value::local_cell(Value::EMPTY_LIST);
                    env.push(empty_cell);
                }

                let env_rc = std::rc::Rc::new(env);

                // Enter coroutine context
                vm.enter_coroutine(co.clone());

                // Execute the closure with coroutine support
                let result = vm.execute_bytecode_coroutine(&bytecode, &constants, Some(&env_rc));

                // Check for pending yield from yield-from delegation BEFORE exiting
                // coroutine context. If there's a pending yield, the coroutine should
                // be suspended, not done.
                if let Some(yielded_value) = vm.take_pending_yield() {
                    // yield-from triggered a yield - coroutine is suspended
                    vm.exit_coroutine();
                    let mut borrowed = co.borrow_mut();
                    borrowed.state = CoroutineState::Suspended;
                    borrowed.yielded_value = Some(yielded_value);
                    // Note: delegate is already set by yield-from
                    // No continuation needed - delegation handles resume
                    return Ok(yielded_value);
                }

                // Exit coroutine context
                vm.exit_coroutine();

                // Re-borrow to update state
                let mut borrowed = co.borrow_mut();
                match result {
                    Ok(VmResult::Done(value)) => {
                        // Check if the coroutine body raised an uncaught exception.
                        // If so, leave it on vm.current_exception for handler-case
                        // to catch, and transition the coroutine to Error state.
                        if vm.current_exception.is_some() {
                            let msg = vm
                                .current_exception
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_default();
                            borrowed.state = CoroutineState::Error(msg);
                            Ok(Value::NIL)
                        } else {
                            borrowed.state = CoroutineState::Done;
                            borrowed.yielded_value = Some(value);
                            Ok(value)
                        }
                    }
                    Ok(VmResult::Yielded {
                        value,
                        continuation,
                    }) => {
                        // Coroutine yielded - save the continuation for later resume
                        borrowed.state = CoroutineState::Suspended;
                        borrowed.yielded_value = Some(value);
                        // Store the first-class continuation for resume_continuation
                        borrowed.saved_value_continuation = Some(continuation);
                        Ok(value)
                    }
                    Err(e) => {
                        borrowed.state = CoroutineState::Error(e.clone());
                        Err(e.into())
                    }
                }
            }
            CoroutineState::Suspended => {
                // Resume using saved first-class continuation
                let continuation = borrowed
                    .saved_value_continuation
                    .take()
                    .ok_or("Suspended coroutine has no saved continuation".to_string())?;

                borrowed.state = CoroutineState::Running;
                drop(borrowed); // Release borrow before VM call

                vm.enter_coroutine(co.clone());
                let result = vm.resume_continuation(continuation, resume_value);
                vm.exit_coroutine();

                let mut borrowed = co.borrow_mut();
                match result {
                    Ok(VmResult::Done(value)) => {
                        if vm.current_exception.is_some() {
                            let msg = vm
                                .current_exception
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_default();
                            borrowed.state = CoroutineState::Error(msg);
                            Ok(Value::NIL)
                        } else {
                            borrowed.state = CoroutineState::Done;
                            borrowed.saved_value_continuation = None;
                            borrowed.yielded_value = Some(value);
                            Ok(value)
                        }
                    }
                    Ok(VmResult::Yielded {
                        value,
                        continuation: new_cont,
                    }) => {
                        borrowed.state = CoroutineState::Suspended;
                        borrowed.saved_value_continuation = Some(new_cont);
                        borrowed.yielded_value = Some(value);
                        Ok(value)
                    }
                    Err(e) => {
                        borrowed.state = CoroutineState::Error(e.clone());
                        Err(e.into())
                    }
                }
            }
            CoroutineState::Running => {
                let cond = Condition::error("coroutine-resume: coroutine is already running");
                vm.current_exception = Some(std::rc::Rc::new(cond));
                Ok(Value::NIL)
            }
            CoroutineState::Done => {
                let cond = Condition::error("coroutine-resume: cannot resume completed coroutine");
                vm.current_exception = Some(std::rc::Rc::new(cond));
                Ok(Value::NIL)
            }
            CoroutineState::Error(e) => {
                let cond = Condition::error(format!(
                    "coroutine-resume: cannot resume errored coroutine: {}",
                    e
                ));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                Ok(Value::NIL)
            }
        }
    } else {
        let cond = Condition::type_error(format!(
            "coroutine-resume: expected coroutine, got {}",
            args[0].type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        Ok(Value::NIL)
    }
}

/// F5: Delegate to a sub-coroutine
///
/// (yield-from co) -> value
///
/// Yields all values from the sub-coroutine until it completes,
/// then returns the sub-coroutine's final value.
///
/// When a coroutine executes `(yield-from sub-coroutine)`:
/// 1. The outer coroutine sets `delegate` to the sub-coroutine
/// 2. The sub-coroutine is resumed once to get its first yielded value
/// 3. The outer coroutine yields that value (suspends via pending_yield)
/// 4. Subsequent resumes of the outer coroutine transparently forward to the delegate
/// 5. When the delegate completes, the outer coroutine continues after yield-from
pub fn prim_yield_from(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.len() != 1 {
        let cond = Condition::arity_error(format!(
            "yield-from: expected 1 argument, got {}",
            args.len()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    if let Some(sub_co) = args[0].as_coroutine() {
        // Check sub-coroutine state
        let state = {
            let borrowed = sub_co.borrow();
            borrowed.state.clone()
        };

        match &state {
            CoroutineState::Done => {
                // Sub-coroutine already done - just return its final value
                let borrowed = sub_co.borrow();
                Ok(borrowed.yielded_value.unwrap_or(Value::EMPTY_LIST))
            }
            CoroutineState::Error(e) => {
                let cond = Condition::error(format!("yield-from: sub-coroutine errored: {}", e));
                vm.current_exception = Some(std::rc::Rc::new(cond));
                Ok(Value::NIL)
            }
            CoroutineState::Running => {
                let cond = Condition::error("yield-from: sub-coroutine is already running");
                vm.current_exception = Some(std::rc::Rc::new(cond));
                Ok(Value::NIL)
            }
            CoroutineState::Created | CoroutineState::Suspended => {
                // Get the outer (current) coroutine
                let outer_co = match vm.current_coroutine() {
                    Some(co) => co.clone(),
                    None => {
                        let cond = Condition::error("yield-from: not inside a coroutine");
                        vm.current_exception = Some(std::rc::Rc::new(cond));
                        return Ok(Value::NIL);
                    }
                };

                // Resume the sub-coroutine once to get its first value
                let result = prim_coroutine_resume(&[args[0]], vm)?;

                // Check sub-coroutine state after resume
                let state_after = {
                    let borrowed = sub_co.borrow();
                    borrowed.state.clone()
                };

                match state_after {
                    CoroutineState::Done => {
                        // Sub-coroutine completed immediately - return its final value
                        // No delegation needed
                        Ok(result)
                    }
                    CoroutineState::Suspended => {
                        // Sub-coroutine yielded - set up delegation
                        {
                            let mut borrowed = outer_co.borrow_mut();
                            borrowed.delegate = Some(args[0]);
                        }

                        // Trigger a yield from the outer coroutine using pending_yield
                        // The VM's instruction loop will check for this and create a
                        // proper yield with continuation capture
                        vm.set_pending_yield(result);

                        // Return the result (will be ignored since pending_yield triggers yield)
                        Ok(result)
                    }
                    CoroutineState::Error(e) => {
                        // Sub-coroutine errored during resume
                        let cond =
                            Condition::error(format!("yield-from: sub-coroutine errored: {}", e));
                        vm.current_exception = Some(std::rc::Rc::new(cond));
                        Ok(Value::NIL)
                    }
                    _ => {
                        // Unexpected state
                        Ok(result)
                    }
                }
            }
        }
    } else {
        let cond = Condition::type_error(format!(
            "yield-from: expected coroutine, got {}",
            args[0].type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        Ok(Value::NIL)
    }
}

/// F6: Get an iterator from a coroutine
///
/// (coroutine->iterator co) -> iterator
///
/// Creates an iterator that yields values from the coroutine.
pub fn prim_coroutine_to_iterator(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coroutine->iterator: expected 1 argument, got {}",
            args.len()
        )));
    }

    if args[0].is_coroutine() {
        // For now, just return the coroutine itself
        // The for loop implementation will need to recognize coroutines
        Ok(args[0])
    } else {
        Err(Condition::type_error(format!(
            "coroutine->iterator: expected coroutine, got {}",
            args[0].type_name()
        )))
    }
}

/// Get the next value from a coroutine iterator
///
/// (coroutine-next co) -> (value . done?)
///
/// Returns a pair of (value, done-flag).
pub fn prim_coroutine_next(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.len() != 1 {
        let cond = Condition::arity_error(format!(
            "coroutine-next: expected 1 argument, got {}",
            args.len()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    if let Some(co) = args[0].as_coroutine() {
        let is_done = {
            let borrowed = co.borrow();
            matches!(borrowed.state, CoroutineState::Done)
        };

        if is_done {
            // Return (nil . #t) to indicate done
            Ok(crate::value::cons(Value::EMPTY_LIST, Value::bool(true)))
        } else {
            // Resume and get next value
            let result = prim_coroutine_resume(args, vm)?;

            // Check if done after resume
            // For now, assume not done unless we got an error
            Ok(crate::value::cons(result, Value::bool(false)))
        }
    } else {
        let cond = Condition::type_error(format!(
            "coroutine-next: expected coroutine, got {}",
            args[0].type_name()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        Ok(Value::NIL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::value::{Arity, Closure};
    use crate::vm::VM;
    use std::rc::Rc;

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

        Value::closure(Closure {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![Value::NIL]),
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
            #[cfg(feature = "jit")]
            jit_code: None,
            #[cfg(feature = "jit")]
            lir_function: None,
        })
    }

    #[test]
    fn test_make_coroutine() {
        let closure = make_test_closure();
        let result = prim_make_coroutine(&[closure]);
        assert!(result.is_ok());

        let result_val = result.unwrap();
        if let Some(co) = result_val.as_coroutine() {
            let borrowed = co.borrow();
            assert!(matches!(borrowed.state, CoroutineState::Created));
        } else {
            panic!("Expected coroutine");
        }
    }

    #[test]
    fn test_make_coroutine_wrong_type() {
        let result = prim_make_coroutine(&[Value::int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_coroutine_status() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let status = prim_coroutine_status(&[co]).unwrap();
        assert_eq!(status, Value::string("created"));
    }

    #[test]
    fn test_coroutine_done() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let done = prim_coroutine_done(&[co]).unwrap();
        assert_eq!(done, Value::bool(false));
    }

    #[test]
    fn test_is_coroutine() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();

        assert_eq!(prim_is_coroutine(&[co]).unwrap(), Value::bool(true));
        assert_eq!(
            prim_is_coroutine(&[Value::int(42)]).unwrap(),
            Value::bool(false)
        );
    }

    #[test]
    fn test_coroutine_value() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let value = prim_coroutine_value(&[co]).unwrap();
        assert_eq!(value, Value::NIL);
    }

    #[test]
    fn test_coroutine_resume_wrong_type() {
        let mut vm = VM::new();
        let result = prim_coroutine_resume(&[Value::int(42)], &mut vm);
        // Now returns Ok(NIL) with current_exception set
        assert!(result.is_ok());
        assert!(vm.current_exception.is_some());
    }

    #[test]
    fn test_coroutine_resume_created() {
        let mut vm = VM::new();
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let result = prim_coroutine_resume(&[co], &mut vm);
        // Should succeed with empty bytecode returning nil
        assert!(result.is_ok());
    }

    #[test]
    fn test_yield_from_wrong_type() {
        let mut vm = VM::new();
        let result = prim_yield_from(&[Value::int(42)], &mut vm);
        // Now returns Ok(NIL) with current_exception set
        assert!(result.is_ok());
        assert!(vm.current_exception.is_some());
    }

    #[test]
    fn test_coroutine_to_iterator() {
        let closure = make_test_closure();
        let co = prim_make_coroutine(&[closure]).unwrap();
        let iter = prim_coroutine_to_iterator(std::slice::from_ref(&co)).unwrap();
        assert!(iter.is_coroutine());
    }

    #[test]
    fn test_coroutine_next_wrong_type() {
        let mut vm = VM::new();
        let result = prim_coroutine_next(&[Value::int(42)], &mut vm);
        // Now returns Ok(NIL) with current_exception set
        assert!(result.is_ok());
        assert!(vm.current_exception.is_some());
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
        if let Some(cons) = result.unwrap().as_cons() {
            // The first element should be the value (nil in this case)
            assert_eq!(cons.first, Value::NIL);
            // The second element should be a boolean (done flag)
            assert!(cons.rest.is_bool());
        } else {
            panic!("Expected cons pair");
        }
    }
}
