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

use crate::compiler::cps::primitives::old_value_to_new;
use crate::compiler::cps::{
    Action, Continuation, CpsInterpreter, CpsTransformer, Trampoline, TrampolineResult,
};
use crate::effects::EffectContext;
use crate::error::LResult;
use crate::value::{Condition, Coroutine, CoroutineState, Value};
use crate::value_old::Value as OldValue;
use crate::vm::{VmResult, VM};
use std::cell::RefMut;
use std::rc::Rc;

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
        let coroutine = Coroutine::new((*c).clone());
        Ok(Value::coroutine(coroutine))
    } else if let Some(jc) = args[0].as_jit_closure() {
        if let Some(source) = &jc.source {
            let coroutine = Coroutine::new(source.clone());
            Ok(Value::coroutine(coroutine))
        } else {
            Err(Condition::error(
                "make-coroutine: JitClosure has no source for coroutine",
            ))
        }
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
        Ok(old_value_to_new(
            &borrowed.yielded_value.clone().unwrap_or(OldValue::Nil),
        ))
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
        // Borrow mutably to update the coroutine state
        let mut borrowed = co.borrow_mut();

        match &borrowed.state {
            CoroutineState::Created => {
                // Check if this closure yields and has source AST for CPS execution
                // We re-infer the effect at runtime because the closure might call
                // functions that weren't defined at compile time
                let use_cps = if let Some(ast) = &borrowed.closure.source_ast {
                    // Re-infer effect with current global definitions
                    let mut effect_ctx = EffectContext::new();
                    register_global_effects(&mut effect_ctx, vm);
                    let runtime_effect = effect_ctx.infer(&ast.body);
                    runtime_effect.may_yield()
                } else {
                    borrowed.closure.effect.may_yield()
                };

                if use_cps {
                    // Use CPS execution path
                    return execute_coroutine_cps(co.clone(), borrowed, vm);
                }

                // Fall back to bytecode execution path
                // First resume - start execution
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
                            borrowed.yielded_value = Some(new_value_to_old(value));
                            Ok(value)
                        }
                    }
                    Ok(VmResult::Yielded(value)) => {
                        // Coroutine yielded - state already updated by Yield instruction
                        borrowed.yielded_value = Some(new_value_to_old(value));
                        Ok(value)
                    }
                    Err(e) => {
                        borrowed.state = CoroutineState::Error(e.clone());
                        Err(e.into())
                    }
                }
            }
            CoroutineState::Suspended => {
                // Check if we have a saved CPS continuation
                if let Some(continuation) = borrowed.saved_continuation.clone() {
                    return resume_coroutine_cps(
                        co.clone(),
                        borrowed,
                        continuation,
                        resume_value,
                        vm,
                    );
                }

                // Fall back to bytecode resumption
                // Resume from suspension
                let context = borrowed
                    .saved_context
                    .clone()
                    .ok_or("Suspended coroutine has no saved context")?;
                let bytecode = borrowed.closure.bytecode.clone();
                let constants = borrowed.closure.constants.clone();

                // Release the borrow before calling resume_from_context
                drop(borrowed);

                // Enter coroutine context so resume_from_context can access current_coroutine()
                vm.enter_coroutine(co.clone());

                // Resume from the saved context
                let result = vm.resume_from_context(context, resume_value, &bytecode, &constants);

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
                            borrowed.yielded_value = Some(new_value_to_old(value));
                            Ok(value)
                        }
                    }
                    Ok(VmResult::Yielded(value)) => {
                        // Coroutine yielded again - state already updated by Yield instruction
                        borrowed.yielded_value = Some(new_value_to_old(value));
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

/// Execute a coroutine using the CPS path
///
/// This is used when the closure has a yielding effect and has source AST available.
fn execute_coroutine_cps(
    co: Rc<std::cell::RefCell<Coroutine>>,
    mut borrowed: RefMut<Coroutine>,
    vm: &mut VM,
) -> LResult<Value> {
    borrowed.state = CoroutineState::Running;

    // Get closure info
    let closure = borrowed.closure.clone();
    let closure_env = closure.env.clone();
    let num_locals = closure.num_locals;
    let num_captures = closure.num_captures;

    // Get the AST from source_ast
    let ast = closure
        .source_ast
        .as_ref()
        .ok_or("Closure has no source AST for CPS execution")?;

    // Set up the environment with RefCell for mutability
    let mut env = (*closure_env).clone();
    let num_locally_defined = num_locals.saturating_sub(num_captures);

    for _ in env.len()..num_captures + num_locally_defined {
        let empty_cell = Value::local_cell(Value::EMPTY_LIST);
        env.push(empty_cell);
    }

    // Use RefCell for shared mutable environment
    let env_rc = std::rc::Rc::new(std::cell::RefCell::new(env));

    // Create effect context and register effects from VM globals
    let mut effect_ctx = EffectContext::new();
    register_global_effects(&mut effect_ctx, vm);

    // Transform the lambda body to CPS
    let mut transformer = CpsTransformer::new(&effect_ctx);
    let cps_body = transformer.transform(&ast.body, Continuation::done());

    // Release borrow before execution
    drop(borrowed);

    // Push coroutine onto stack so bytecode VM knows we're in a coroutine context
    vm.coroutine_stack.push(co.clone());

    // Create interpreter and evaluate
    let mut interpreter = CpsInterpreter::new(vm, env_rc.clone());
    let initial_action = match interpreter.eval(&cps_body) {
        Ok(action) => action,
        Err(e) => {
            // If eval fails, we need to clean up the coroutine state
            // Pop coroutine from stack
            vm.coroutine_stack.pop();

            // Update coroutine state to Error
            let mut borrowed = co.borrow_mut();
            borrowed.state = CoroutineState::Error(e.clone());
            return Err(e.into());
        }
    };

    // Run trampoline
    let mut trampoline = Trampoline::new();
    let result = trampoline.run_with_vm(initial_action, vm, &env_rc);

    // Pop coroutine from stack
    vm.coroutine_stack.pop();

    // Update coroutine state
    let mut borrowed = co.borrow_mut();
    match result {
        TrampolineResult::Done(value) => {
            borrowed.state = CoroutineState::Done;
            borrowed.yielded_value = Some(new_value_to_old(value));
            borrowed.saved_continuation = None;
            borrowed.saved_env = None;
            Ok(value)
        }
        TrampolineResult::Suspended {
            value,
            continuation,
        } => {
            borrowed.state = CoroutineState::Suspended;
            borrowed.yielded_value = Some(new_value_to_old(value));
            borrowed.saved_continuation = Some(continuation);
            // Save the environment so local variables persist across yields
            // Convert the environment from new Values to old Values
            let borrowed_env = env_rc.borrow();
            let old_env: Vec<OldValue> =
                borrowed_env.iter().map(|v| new_value_to_old(*v)).collect();
            borrowed.saved_env = Some(std::rc::Rc::new(std::cell::RefCell::new(old_env)));
            Ok(value)
        }
        TrampolineResult::Error(e) => {
            borrowed.state = CoroutineState::Error(e.clone());
            Err(e.into())
        }
    }
}

/// Resume a coroutine using the CPS path
///
/// This is used when the coroutine was suspended with a saved CPS continuation.
fn resume_coroutine_cps(
    co: Rc<std::cell::RefCell<Coroutine>>,
    mut borrowed: RefMut<Coroutine>,
    continuation: Rc<Continuation>,
    resume_value: Value,
    vm: &mut VM,
) -> LResult<Value> {
    borrowed.state = CoroutineState::Running;
    // Use saved environment if available, otherwise create from closure's environment
    let env = borrowed.saved_env.clone().unwrap_or_else(|| {
        // Convert immutable closure env to mutable RefCell env with old Values
        let old_env: Vec<OldValue> = borrowed
            .closure
            .env
            .iter()
            .map(|v| new_value_to_old(*v))
            .collect();
        Rc::new(std::cell::RefCell::new(old_env))
    });

    // Release borrow
    drop(borrowed);

    // Push coroutine onto stack so bytecode VM knows we're in a coroutine context
    vm.coroutine_stack.push(co.clone());

    // Apply resume value to continuation
    // Convert environment back to new Values for the CPS interpreter
    let borrowed_env = env.borrow();
    let new_env: Vec<Value> = borrowed_env.iter().map(old_value_to_new).collect();
    let new_env_rc = std::rc::Rc::new(std::cell::RefCell::new(new_env));

    let mut trampoline = Trampoline::new();
    let initial_action = Action::return_value(resume_value, continuation);
    let result = trampoline.run_with_vm(initial_action, vm, &new_env_rc);

    // Pop coroutine from stack
    vm.coroutine_stack.pop();

    // Update coroutine state
    let mut borrowed = co.borrow_mut();
    match result {
        TrampolineResult::Done(value) => {
            borrowed.state = CoroutineState::Done;
            borrowed.yielded_value = Some(new_value_to_old(value));
            borrowed.saved_continuation = None;
            borrowed.saved_env = None;
            Ok(value)
        }
        TrampolineResult::Suspended {
            value,
            continuation,
        } => {
            borrowed.state = CoroutineState::Suspended;
            borrowed.yielded_value = Some(new_value_to_old(value));
            borrowed.saved_continuation = Some(continuation);
            // Keep the saved environment
            Ok(value)
        }
        TrampolineResult::Error(e) => {
            borrowed.state = CoroutineState::Error(e.clone());
            Err(e.into())
        }
    }
}

/// Register effects of global functions from VM globals
///
/// This scans the VM's globals and registers the effect of each closure.
/// Native functions are assumed to be pure.
fn register_global_effects(effect_ctx: &mut EffectContext, vm: &VM) {
    use crate::effects::Effect;
    use crate::value::SymbolId;

    for (&sym_id, value) in &vm.globals {
        let effect = if let Some(c) = value.as_closure() {
            c.effect
        } else if value.as_native_fn().is_some() || value.as_vm_aware_fn().is_some() {
            Effect::Pure
        } else {
            continue; // Skip non-function values
        };
        effect_ctx.register_global(SymbolId(sym_id), effect);
    }
}

/// F5: Delegate to a sub-coroutine
///
/// (yield-from co) -> value
///
/// Yields all values from the sub-coroutine until it completes,
/// then returns the sub-coroutine's final value.
pub fn prim_yield_from(args: &[Value], vm: &mut VM) -> LResult<Value> {
    if args.len() != 1 {
        let cond = Condition::arity_error(format!(
            "yield-from: expected 1 argument, got {}",
            args.len()
        ));
        vm.current_exception = Some(std::rc::Rc::new(cond));
        return Ok(Value::NIL);
    }

    if let Some(co) = args[0].as_coroutine() {
        // Resume the sub-coroutine once
        let state = {
            let borrowed = co.borrow();
            borrowed.state.clone()
        };

        match &state {
            CoroutineState::Created | CoroutineState::Suspended => {
                // Resume the coroutine once
                prim_coroutine_resume(&[args[0]], vm)
            }
            CoroutineState::Done => {
                let borrowed = co.borrow();
                Ok(old_value_to_new(
                    &borrowed.yielded_value.clone().unwrap_or(OldValue::Nil),
                ))
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

/// Convert a new Value to an old Value
/// This is a temporary function for the migration period
pub fn new_value_to_old(val: Value) -> OldValue {
    use crate::value::heap::{deref, HeapObject};

    if let Some(b) = val.as_bool() {
        OldValue::Bool(b)
    } else if let Some(i) = val.as_int() {
        OldValue::Int(i)
    } else if let Some(f) = val.as_float() {
        OldValue::Float(f)
    } else if let Some(sym_id) = val.as_symbol() {
        OldValue::Symbol(crate::value::SymbolId(sym_id))
    } else if let Some(kw_id) = val.as_keyword() {
        OldValue::Keyword(crate::value::SymbolId(kw_id))
    } else if val.is_nil() {
        OldValue::Nil
    } else if val.is_heap() {
        unsafe {
            match deref(val) {
                HeapObject::String(s) => OldValue::String(s.clone().into()),
                HeapObject::Cons(cons) => {
                    OldValue::Cons(std::rc::Rc::new(crate::value_old::Cons {
                        first: new_value_to_old(cons.first),
                        rest: new_value_to_old(cons.rest),
                    }))
                }
                HeapObject::Vector(v) => {
                    let borrowed = v.borrow();
                    let old_vals: Vec<OldValue> =
                        borrowed.iter().map(|v| new_value_to_old(*v)).collect();
                    OldValue::Vector(std::rc::Rc::new(old_vals))
                }
                HeapObject::Table(t) => {
                    let borrowed = t.borrow();
                    let mut old_table = std::collections::BTreeMap::new();
                    for (k, v) in borrowed.iter() {
                        old_table.insert(k.clone(), new_value_to_old(*v));
                    }
                    OldValue::Table(std::rc::Rc::new(std::cell::RefCell::new(old_table)))
                }
                HeapObject::Struct(s) => {
                    let mut old_struct = std::collections::BTreeMap::new();
                    for (k, v) in s.iter() {
                        old_struct.insert(k.clone(), new_value_to_old(*v));
                    }
                    OldValue::Struct(std::rc::Rc::new(old_struct))
                }
                HeapObject::Closure(_) => {
                    // For now, we can't convert closures
                    OldValue::Nil
                }
                HeapObject::JitClosure(_) => {
                    // For now, we can't convert JIT closures
                    OldValue::Nil
                }
                HeapObject::Condition(_) => {
                    // For now, we can't convert conditions
                    OldValue::Nil
                }
                HeapObject::Coroutine(_) => {
                    // For now, we can't convert coroutines
                    OldValue::Nil
                }
                HeapObject::Cell(c, _) => {
                    let borrowed = c.borrow();
                    OldValue::Cell(std::rc::Rc::new(std::cell::RefCell::new(Box::new(
                        new_value_to_old(*borrowed),
                    ))))
                }
                HeapObject::Float(_) => {
                    // Float values should be handled above
                    OldValue::Nil
                }
                HeapObject::NativeFn(_) => {
                    // For now, we can't convert native functions
                    OldValue::Nil
                }
                HeapObject::VmAwareFn(_) => {
                    // For now, we can't convert VM-aware functions
                    OldValue::Nil
                }
                _ => OldValue::Nil,
            }
        }
    } else {
        OldValue::Nil
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
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

        Value::closure(Closure {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![Value::NIL]),
            source_ast: None,
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
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
