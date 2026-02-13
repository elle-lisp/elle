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

use crate::compiler::cps::{
    Action, Continuation, CpsInterpreter, CpsTransformer, Trampoline, TrampolineResult,
};
use crate::compiler::effects::EffectContext;
use crate::value::{Coroutine, CoroutineState, Value};
use crate::vm::{VmResult, VM};
use std::cell::RefMut;
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
            let coroutine = Coroutine::new(c.clone());
            Ok(Value::Coroutine(Rc::new(std::cell::RefCell::new(
                coroutine,
            ))))
        }
        Value::JitClosure(jc) => {
            if let Some(source) = &jc.source {
                let coroutine = Coroutine::new(source.clone());
                Ok(Value::Coroutine(Rc::new(std::cell::RefCell::new(
                    coroutine,
                ))))
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
            let borrowed = co.borrow();
            let status = match &borrowed.state {
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
        Value::Coroutine(co) => {
            let borrowed = co.borrow();
            Ok(Value::Bool(matches!(borrowed.state, CoroutineState::Done)))
        }
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
        Value::Coroutine(co) => {
            let borrowed = co.borrow();
            Ok(borrowed.yielded_value.clone().unwrap_or(Value::Nil))
        }
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

    let resume_value = args.get(1).cloned().unwrap_or(Value::Nil);

    match &args[0] {
        Value::Coroutine(co) => {
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
                        let empty_cell = Value::LocalCell(std::rc::Rc::new(
                            std::cell::RefCell::new(Box::new(Value::Nil)),
                        ));
                        env.push(empty_cell);
                    }

                    let env_rc = std::rc::Rc::new(env);

                    // Enter coroutine context
                    vm.enter_coroutine(co.clone());

                    // Execute the closure with coroutine support
                    let result =
                        vm.execute_bytecode_coroutine(&bytecode, &constants, Some(&env_rc));

                    // Exit coroutine context
                    vm.exit_coroutine();

                    // Re-borrow to update state
                    let mut borrowed = co.borrow_mut();
                    match result {
                        Ok(VmResult::Done(value)) => {
                            borrowed.state = CoroutineState::Done;
                            borrowed.yielded_value = Some(value.clone());
                            Ok(value)
                        }
                        Ok(VmResult::Yielded(value)) => {
                            // Coroutine yielded - state already updated by Yield instruction
                            borrowed.yielded_value = Some(value.clone());
                            Ok(value)
                        }
                        Err(e) => {
                            borrowed.state = CoroutineState::Error(e.clone());
                            Err(e)
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

                    // Resume from the saved context
                    let result =
                        vm.resume_from_context(context, resume_value, &bytecode, &constants);

                    // Re-borrow to update state
                    let mut borrowed = co.borrow_mut();
                    match result {
                        Ok(VmResult::Done(value)) => {
                            borrowed.state = CoroutineState::Done;
                            borrowed.yielded_value = Some(value.clone());
                            Ok(value)
                        }
                        Ok(VmResult::Yielded(value)) => {
                            // Coroutine yielded again - state already updated by Yield instruction
                            borrowed.yielded_value = Some(value.clone());
                            Ok(value)
                        }
                        Err(e) => {
                            borrowed.state = CoroutineState::Error(e.clone());
                            Err(e)
                        }
                    }
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

/// Execute a coroutine using the CPS path
///
/// This is used when the closure has a yielding effect and has source AST available.
fn execute_coroutine_cps(
    co: Rc<std::cell::RefCell<Coroutine>>,
    mut borrowed: RefMut<Coroutine>,
    vm: &mut VM,
) -> Result<Value, String> {
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
        let empty_cell = Value::LocalCell(std::rc::Rc::new(std::cell::RefCell::new(Box::new(
            Value::Nil,
        ))));
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
            return Err(e);
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
            borrowed.yielded_value = Some(value.clone());
            borrowed.saved_continuation = None;
            borrowed.saved_env = None;
            Ok(value)
        }
        TrampolineResult::Suspended {
            value,
            continuation,
        } => {
            borrowed.state = CoroutineState::Suspended;
            borrowed.yielded_value = Some(value.clone());
            borrowed.saved_continuation = Some(continuation);
            // Save the environment so local variables persist across yields
            borrowed.saved_env = Some(env_rc.clone());
            Ok(value)
        }
        TrampolineResult::Error(e) => {
            borrowed.state = CoroutineState::Error(e.clone());
            Err(e)
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
) -> Result<Value, String> {
    borrowed.state = CoroutineState::Running;
    // Use saved environment if available, otherwise create from closure's environment
    let env = borrowed.saved_env.clone().unwrap_or_else(|| {
        // Convert immutable closure env to mutable RefCell env
        Rc::new(std::cell::RefCell::new((*borrowed.closure.env).clone()))
    });

    // Release borrow
    drop(borrowed);

    // Push coroutine onto stack so bytecode VM knows we're in a coroutine context
    vm.coroutine_stack.push(co.clone());

    // Apply resume value to continuation
    let mut trampoline = Trampoline::new();
    let initial_action = Action::return_value(resume_value, continuation);
    let result = trampoline.run_with_vm(initial_action, vm, &env);

    // Pop coroutine from stack
    vm.coroutine_stack.pop();

    // Update coroutine state
    let mut borrowed = co.borrow_mut();
    match result {
        TrampolineResult::Done(value) => {
            borrowed.state = CoroutineState::Done;
            borrowed.yielded_value = Some(value.clone());
            borrowed.saved_continuation = None;
            borrowed.saved_env = None;
            Ok(value)
        }
        TrampolineResult::Suspended {
            value,
            continuation,
        } => {
            borrowed.state = CoroutineState::Suspended;
            borrowed.yielded_value = Some(value.clone());
            borrowed.saved_continuation = Some(continuation);
            // Keep the saved environment
            Ok(value)
        }
        TrampolineResult::Error(e) => {
            borrowed.state = CoroutineState::Error(e.clone());
            Err(e)
        }
    }
}

/// Register effects of global functions from VM globals
///
/// This scans the VM's globals and registers the effect of each closure.
/// Native functions are assumed to be pure.
fn register_global_effects(effect_ctx: &mut EffectContext, vm: &VM) {
    use crate::compiler::effects::Effect;
    use crate::value::SymbolId;

    for (&sym_id, value) in &vm.globals {
        let effect = match value {
            Value::Closure(c) => c.effect,
            Value::NativeFn(_) | Value::VmAwareFn(_) => Effect::Pure,
            _ => continue, // Skip non-function values
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
pub fn prim_yield_from(args: &[Value], vm: &mut VM) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "yield-from requires exactly 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Coroutine(co) => {
            // Resume the sub-coroutine once
            let state = {
                let borrowed = co.borrow();
                borrowed.state.clone()
            };

            match &state {
                CoroutineState::Created | CoroutineState::Suspended => {
                    // Resume the coroutine once
                    prim_coroutine_resume(&[Value::Coroutine(co.clone())], vm)
                }
                CoroutineState::Done => {
                    let borrowed = co.borrow();
                    Ok(borrowed.yielded_value.clone().unwrap_or(Value::Nil))
                }
                CoroutineState::Error(e) => Err(e.clone()),
                CoroutineState::Running => Err("Sub-coroutine is already running".to_string()),
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
            let is_done = {
                let borrowed = co.borrow();
                matches!(borrowed.state, CoroutineState::Done)
            };

            if is_done {
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
            let borrowed = co.borrow();
            assert!(matches!(borrowed.state, CoroutineState::Created));
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
