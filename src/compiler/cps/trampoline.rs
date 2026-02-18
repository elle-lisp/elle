//! Trampoline executor for CPS code
//!
//! The trampoline is the runtime loop that drives CPS execution.
//! It repeatedly processes Actions until completion or yield.

use super::{Action, Continuation, CpsInterpreter, CpsTransformer};
use crate::effects::EffectContext;
use crate::value::Value;
use crate::vm::VM;
use std::cell::RefCell;
use std::rc::Rc;

/// Result of trampoline execution
#[derive(Debug, Clone)]
pub enum TrampolineResult {
    /// Execution completed with a value
    Done(Value),
    /// Execution suspended (yielded)
    Suspended {
        /// Value yielded
        value: Value,
        /// Continuation to resume
        continuation: Rc<Continuation>,
    },
    /// Execution failed
    Error(String),
}

impl TrampolineResult {
    /// Check if execution completed
    pub fn is_done(&self) -> bool {
        matches!(self, TrampolineResult::Done(_))
    }

    /// Check if execution suspended
    pub fn is_suspended(&self) -> bool {
        matches!(self, TrampolineResult::Suspended { .. })
    }

    /// Get the value if done
    pub fn value(&self) -> Option<&Value> {
        match self {
            TrampolineResult::Done(v) => Some(v),
            TrampolineResult::Suspended { value, .. } => Some(value),
            TrampolineResult::Error(_) => None,
        }
    }

    /// Convert to Result
    pub fn into_result(self) -> Result<Value, String> {
        match self {
            TrampolineResult::Done(v) => Ok(v),
            TrampolineResult::Suspended { value, .. } => Ok(value),
            TrampolineResult::Error(e) => Err(e),
        }
    }
}

/// Configuration for the trampoline
#[derive(Debug, Clone)]
pub struct TrampolineConfig {
    /// Maximum number of steps before forcing a yield (prevents infinite loops)
    pub max_steps: usize,
}

impl Default for TrampolineConfig {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
        }
    }
}

/// The trampoline executor
pub struct Trampoline {
    config: TrampolineConfig,
    step_count: usize,
}

impl Trampoline {
    /// Create a new trampoline with default config
    pub fn new() -> Self {
        Self {
            config: TrampolineConfig::default(),
            step_count: 0,
        }
    }

    /// Create a trampoline with custom config
    pub fn with_config(config: TrampolineConfig) -> Self {
        Self {
            config,
            step_count: 0,
        }
    }

    /// Run the trampoline starting with an action
    pub fn run(&mut self, initial_action: Action) -> TrampolineResult {
        let mut current_action = initial_action;
        self.step_count = 0;

        loop {
            self.step_count += 1;

            if self.step_count > self.config.max_steps {
                return TrampolineResult::Error(format!(
                    "Trampoline exceeded max steps ({})",
                    self.config.max_steps
                ));
            }

            match current_action {
                Action::Done(value) => {
                    return TrampolineResult::Done(value);
                }

                Action::Return {
                    value,
                    continuation,
                } => {
                    // Apply the value to the continuation
                    current_action = self.apply_continuation(value, continuation);
                }

                Action::Yield {
                    value,
                    continuation,
                } => {
                    // Suspend execution
                    return TrampolineResult::Suspended {
                        value,
                        continuation,
                    };
                }

                Action::Call {
                    func,
                    args: _,
                    continuation: _,
                } => {
                    // For now, we can't execute calls without VM access
                    // This will be connected to the VM in a later step
                    return TrampolineResult::Error(format!(
                        "Trampoline Call not yet implemented: {:?}",
                        func
                    ));
                }

                Action::TailCall { func, args: _ } => {
                    // For now, we can't execute tail calls without VM access
                    return TrampolineResult::Error(format!(
                        "Trampoline TailCall not yet implemented: {:?}",
                        func
                    ));
                }

                Action::Error(msg) => {
                    return TrampolineResult::Error(msg);
                }
            }
        }
    }

    /// Apply a value to a continuation, producing the next action
    fn apply_continuation(&self, value: Value, cont: Rc<Continuation>) -> Action {
        match cont.as_ref() {
            Continuation::Done => Action::Done(value),

            Continuation::Sequence { remaining, next } => {
                if remaining.is_empty() {
                    // No more expressions, continue with next
                    Action::return_value(value, next.clone())
                } else {
                    // More expressions to evaluate
                    // For now, return an error - expression evaluation needs VM
                    Action::error("Sequence evaluation not yet implemented")
                }
            }

            Continuation::IfBranch {
                then_branch: _,
                else_branch: _,
                next: _,
            } => {
                // Choose branch based on value truthiness
                // For now, return an error - branch evaluation needs VM
                Action::error("IfBranch evaluation not yet implemented")
            }

            Continuation::LetBinding {
                var: _,
                remaining_bindings: _,
                bound_values: _,
                body: _,
                next: _,
            } => {
                // Bind the value and continue
                // For now, return an error - let evaluation needs VM
                Action::error("LetBinding evaluation not yet implemented")
            }

            Continuation::CallReturn { saved_env: _, next } => {
                // Restore environment and continue
                Action::return_value(value, next.clone())
            }

            Continuation::Apply { cont_fn } => {
                // Apply the continuation function
                cont_fn(value)
            }

            Continuation::CpsSequence { .. } => {
                // CpsSequence requires VM access - this shouldn't be called
                // from the non-VM path
                Action::error("CpsSequence continuation requires VM access")
            }

            Continuation::CpsWhile { .. } => {
                // CpsWhile requires VM access - this shouldn't be called
                // from the non-VM path
                Action::error("CpsWhile continuation requires VM access")
            }

            Continuation::CpsWhileBody { .. } => {
                // CpsWhileBody requires VM access - this shouldn't be called
                // from the non-VM path
                Action::error("CpsWhileBody continuation requires VM access")
            }

            Continuation::CpsSequenceAfterYield { .. } => {
                // CpsSequenceAfterYield requires VM access - this shouldn't be called
                // from the non-VM path
                Action::error("CpsSequenceAfterYield continuation requires VM access")
            }

            Continuation::WithEnv { env: _, inner } => {
                // WithEnv requires VM access for proper environment handling
                // In the non-VM path, just continue with inner
                Action::return_value(value, inner.clone())
            }
        }
    }

    /// Get the number of steps taken in the last run
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    /// Run the trampoline with VM access for function calls
    ///
    /// This variant allows the trampoline to execute function calls
    /// by delegating to the VM.
    pub fn run_with_vm(
        &mut self,
        initial_action: Action,
        vm: &mut VM,
        env: &Rc<RefCell<Vec<Value>>>,
    ) -> TrampolineResult {
        let mut current_action = initial_action;
        self.step_count = 0;

        loop {
            self.step_count += 1;

            if self.step_count > self.config.max_steps {
                return TrampolineResult::Error(format!(
                    "Trampoline exceeded max steps ({})",
                    self.config.max_steps
                ));
            }

            match current_action {
                Action::Done(value) => {
                    return TrampolineResult::Done(value);
                }

                Action::Return {
                    value,
                    continuation,
                } => {
                    current_action = self.apply_continuation_with_vm(value, continuation, vm, env);
                }

                Action::Yield {
                    value,
                    continuation,
                } => {
                    return TrampolineResult::Suspended {
                        value,
                        continuation,
                    };
                }

                Action::Call {
                    func,
                    args,
                    continuation,
                } => {
                    // Check if this is a yielding closure with source AST
                    // If so, use CPS execution to properly handle nested yields
                    if let Some(closure) = func.as_closure() {
                        if closure.effect.may_yield() && closure.source_ast.is_some() {
                            // Use CPS execution for yielding closures
                            match execute_cps_call(closure, &args, continuation.clone(), vm) {
                                Ok(action) => {
                                    current_action = action;
                                    continue;
                                }
                                Err(e) => return TrampolineResult::Error(e),
                            }
                        }
                    }

                    // Fall back to bytecode execution for non-yielding or non-CPS closures
                    match call_value_with_vm_result(&func, &args, vm) {
                        Ok(crate::vm::VmResult::Done(result)) => {
                            current_action = Action::return_value(result, continuation);
                        }
                        Ok(crate::vm::VmResult::Yielded {
                            value,
                            continuation: _,
                        }) => {
                            // Yield happened - propagate it up
                            // Note: We ignore the VM continuation here because CPS has its own
                            return TrampolineResult::Suspended {
                                value,
                                continuation,
                            };
                        }
                        Err(e) => return TrampolineResult::Error(e),
                    }
                }

                Action::TailCall { func, args } => {
                    // Execute tail call via VM
                    match call_value_with_vm(&func, &args, vm) {
                        Ok(result) => {
                            current_action = Action::done(result);
                        }
                        Err(e) => return TrampolineResult::Error(e),
                    }
                }

                Action::Error(msg) => {
                    return TrampolineResult::Error(msg);
                }
            }
        }
    }

    /// Apply a value to a continuation with VM access
    fn apply_continuation_with_vm(
        &mut self,
        value: Value,
        cont: Rc<Continuation>,
        vm: &mut VM,
        env: &Rc<RefCell<Vec<Value>>>,
    ) -> Action {
        match cont.as_ref() {
            Continuation::Done => Action::Done(value),

            Continuation::CallReturn { saved_env, next } => {
                // Restore caller's environment and continue
                // We wrap the next continuation with the saved environment
                // so that subsequent evaluation uses the caller's environment
                let wrapped_next = Rc::new(Continuation::WithEnv {
                    env: saved_env.clone(),
                    inner: next.clone(),
                });
                Action::return_value(value, wrapped_next)
            }

            Continuation::Apply { cont_fn } => {
                // Apply the continuation function
                cont_fn(value)
            }

            Continuation::CpsSequence { remaining, next } => {
                // Resume evaluating remaining CPS expressions
                // The 'value' is the resume value (ignored for sequences)
                if remaining.is_empty() {
                    // No more expressions - continue with next continuation
                    Action::return_value(value, next.clone())
                } else {
                    // Evaluate remaining expressions
                    let mut interpreter = super::CpsInterpreter::new(vm, env.clone());

                    // Evaluate expressions one by one
                    for (i, expr) in remaining.iter().enumerate() {
                        match interpreter.eval(expr) {
                            Ok(action) => {
                                match action {
                                    Action::Done(_) => {
                                        // Continue to next expression
                                        // If this is the last expression, return with next continuation
                                        if i == remaining.len() - 1 {
                                            if let Action::Done(val) = action {
                                                return Action::return_value(val, next.clone());
                                            }
                                        }
                                    }
                                    Action::Yield {
                                        value,
                                        continuation: _,
                                    } => {
                                        // Yield happened - capture remaining expressions
                                        let new_remaining = remaining[i + 1..].to_vec();

                                        let resume_cont = if new_remaining.is_empty() {
                                            // No more expressions after this yield
                                            next.clone()
                                        } else {
                                            // Create new CpsSequence for remaining
                                            Rc::new(Continuation::CpsSequence {
                                                remaining: new_remaining,
                                                next: next.clone(),
                                            })
                                        };

                                        return Action::yield_value(value, resume_cont);
                                    }
                                    other => return other,
                                }
                            }
                            Err(e) => return Action::error(e),
                        }
                    }

                    // Should not reach here
                    Action::return_value(Value::NIL, next.clone())
                }
            }

            Continuation::CpsWhile { cond, body, next } => {
                // Resume a while loop after a yield in the body
                // The 'value' is the resume value (ignored)
                let mut interpreter = super::CpsInterpreter::new(vm, env.clone());

                loop {
                    // Evaluate condition
                    match interpreter.eval(cond) {
                        Ok(action) => {
                            match action {
                                Action::Done(cond_val)
                                | Action::Return {
                                    value: cond_val, ..
                                } => {
                                    if !cond_val.is_truthy() {
                                        // Loop condition is false - exit loop
                                        return Action::return_value(Value::NIL, next.clone());
                                    }
                                }
                                Action::Yield {
                                    value,
                                    continuation: _,
                                } => {
                                    // Yield in condition - capture while state
                                    let while_cont = Rc::new(Continuation::CpsWhile {
                                        cond: cond.clone(),
                                        body: body.clone(),
                                        next: next.clone(),
                                    });
                                    return Action::yield_value(value, while_cont);
                                }
                                other => return other,
                            }
                        }
                        Err(e) => return Action::error(e),
                    }

                    // Evaluate body
                    match interpreter.eval(body) {
                        Ok(action) => {
                            match action {
                                Action::Done(_) | Action::Return { .. } => {
                                    // Body completed, continue loop
                                }
                                Action::Yield {
                                    value,
                                    continuation: body_cont,
                                } => {
                                    // Yield in body - capture while state with body continuation
                                    let resume_cont = if body_cont.is_done() {
                                        // Body is done after yield, continue with while loop
                                        Rc::new(Continuation::CpsWhile {
                                            cond: cond.clone(),
                                            body: body.clone(),
                                            next: next.clone(),
                                        })
                                    } else {
                                        // Body has more to do, then continue with while loop
                                        Rc::new(Continuation::CpsWhileBody {
                                            body_cont,
                                            cond: cond.clone(),
                                            body: body.clone(),
                                            next: next.clone(),
                                        })
                                    };
                                    return Action::yield_value(value, resume_cont);
                                }
                                other => return other,
                            }
                        }
                        Err(e) => return Action::error(e),
                    }
                }
            }

            Continuation::CpsWhileBody {
                body_cont,
                cond,
                body,
                next,
            } => {
                // Resume the rest of the while loop body, then continue the loop
                // First, apply the body continuation
                let mut body_result =
                    self.apply_continuation_with_vm(value, body_cont.clone(), vm, env);

                // Keep applying continuations until we get Done, Yield, or Error
                loop {
                    match body_result {
                        Action::Done(_) => {
                            // Body completed, continue with while loop
                            let while_cont = Rc::new(Continuation::CpsWhile {
                                cond: cond.clone(),
                                body: body.clone(),
                                next: next.clone(),
                            });
                            return Action::return_value(Value::NIL, while_cont);
                        }
                        Action::Return {
                            value: val,
                            continuation: cont,
                        } => {
                            // More continuations to apply
                            body_result = self.apply_continuation_with_vm(val, cont, vm, env);
                        }
                        Action::Yield {
                            value,
                            continuation: new_body_cont,
                        } => {
                            // Another yield in body - capture state
                            let resume_cont = if new_body_cont.is_done() {
                                Rc::new(Continuation::CpsWhile {
                                    cond: cond.clone(),
                                    body: body.clone(),
                                    next: next.clone(),
                                })
                            } else {
                                Rc::new(Continuation::CpsWhileBody {
                                    body_cont: new_body_cont,
                                    cond: cond.clone(),
                                    body: body.clone(),
                                    next: next.clone(),
                                })
                            };
                            return Action::yield_value(value, resume_cont);
                        }
                        other => return other,
                    }
                }
            }

            Continuation::CpsSequenceAfterYield {
                yield_cont,
                remaining_cont,
            } => {
                // First, apply the yield continuation (e.g., continue a while loop)
                let yield_result =
                    self.apply_continuation_with_vm(value, yield_cont.clone(), vm, env);

                match yield_result {
                    Action::Done(val) | Action::Return { value: val, .. } => {
                        // Yield continuation completed, now evaluate remaining expressions
                        Action::return_value(val, remaining_cont.clone())
                    }
                    Action::Yield {
                        value,
                        continuation: new_yield_cont,
                    } => {
                        // Another yield - chain with remaining_cont
                        let chained_cont = Rc::new(Continuation::CpsSequenceAfterYield {
                            yield_cont: new_yield_cont,
                            remaining_cont: remaining_cont.clone(),
                        });
                        Action::yield_value(value, chained_cont)
                    }
                    other => other,
                }
            }

            Continuation::WithEnv {
                env: saved_env,
                inner,
            } => {
                // Continue with restored environment
                self.apply_continuation_with_vm(value, inner.clone(), vm, saved_env)
            }

            // For other continuation types, fall back to the basic apply
            _ => self.apply_continuation(value, cont),
        }
    }
}

/// Execute a CPS call for a yielding closure
///
/// This transforms the closure's body to CPS and executes it, properly
/// chaining the return continuation so that yields work correctly.
fn execute_cps_call(
    closure: &crate::value::Closure,
    args: &[Value],
    return_cont: Rc<Continuation>,
    vm: &mut VM,
) -> Result<Action, String> {
    // Get the AST from source_ast
    let ast = closure
        .source_ast
        .as_ref()
        .ok_or("Closure has no source AST for CPS execution")?;

    // Build the callee's environment
    let mut new_env = Vec::new();
    new_env.extend((*closure.env).iter().cloned());
    new_env.extend(args.iter().cloned());

    // Add local cells for locally-defined variables
    let num_params = match closure.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };
    let num_locally_defined = closure
        .num_locals
        .saturating_sub(num_params + closure.num_captures);

    for _ in 0..num_locally_defined {
        let empty_cell = Value::cell(Value::NIL);
        new_env.push(empty_cell);
    }

    let callee_env = Rc::new(RefCell::new(new_env));

    // Create effect context and register effects from VM globals
    // This allows the CPS transformer to know which functions yield
    let mut effect_ctx = EffectContext::new();
    register_global_effects(&mut effect_ctx, vm);

    // Transform the lambda body to CPS with the return continuation
    // The return continuation will restore the caller's environment when the callee completes
    let mut transformer = CpsTransformer::new(&effect_ctx);
    let cps_body = transformer.transform(&ast.body, return_cont);

    // Create interpreter with callee's environment and evaluate
    let mut interpreter = CpsInterpreter::new(vm, callee_env);
    interpreter.eval(&cps_body)
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

/// Call a value as a function with the given arguments using the VM
fn call_value_with_vm(func: &Value, args: &[Value], vm: &mut VM) -> Result<Value, String> {
    let result = if let Some(closure) = func.as_closure() {
        // Build environment
        let mut new_env = Vec::new();
        new_env.extend((*closure.env).iter().cloned());
        new_env.extend(args.iter().cloned());

        // Add local cells
        let num_params = match closure.arity {
            crate::value::Arity::Exact(n) => n,
            crate::value::Arity::AtLeast(n) => n,
            crate::value::Arity::Range(min, _) => min,
        };
        let num_locally_defined = closure
            .num_locals
            .saturating_sub(num_params + closure.num_captures);

        for _ in 0..num_locally_defined {
            let empty_cell = Value::cell(Value::NIL);
            new_env.push(empty_cell);
        }

        let env_rc = std::rc::Rc::new(new_env);

        // Execute
        vm.execute_bytecode(&closure.bytecode, &closure.constants, Some(&env_rc))
    } else if let Some(f) = func.as_native_fn() {
        f(args).map_err(|e| e.to_string())
    } else if let Some(f) = func.as_vm_aware_fn() {
        f(args, vm).map_err(|e| e.into())
    } else {
        Err(format!("Cannot call {}", func.type_name()))
    };

    // Check for exception set by the function
    if let Some(exc) = vm.current_exception.take() {
        return Err(format!("{}", exc));
    }

    result
}

/// Call a value with VM access, returning VmResult to detect yields
fn call_value_with_vm_result(
    func: &Value,
    args: &[Value],
    vm: &mut VM,
) -> Result<crate::vm::VmResult, String> {
    let result = if let Some(closure) = func.as_closure() {
        // Build environment
        let mut new_env = Vec::new();
        new_env.extend((*closure.env).iter().cloned());
        new_env.extend(args.iter().cloned());

        // Add local cells
        let num_params = match closure.arity {
            crate::value::Arity::Exact(n) => n,
            crate::value::Arity::AtLeast(n) => n,
            crate::value::Arity::Range(min, _) => min,
        };
        let num_locally_defined = closure
            .num_locals
            .saturating_sub(num_params + closure.num_captures);

        for _ in 0..num_locally_defined {
            let empty_cell = Value::cell(Value::NIL);
            new_env.push(empty_cell);
        }

        let env_rc = std::rc::Rc::new(new_env);

        // Execute - use coroutine-aware path if in a coroutine
        if vm.in_coroutine() {
            vm.execute_bytecode_coroutine(&closure.bytecode, &closure.constants, Some(&env_rc))
        } else {
            // For non-coroutine calls, wrap the result
            match vm.execute_bytecode(&closure.bytecode, &closure.constants, Some(&env_rc)) {
                Ok(v) => Ok(crate::vm::VmResult::Done(v)),
                Err(e) => Err(e),
            }
        }
    } else if let Some(f) = func.as_native_fn() {
        match f(args) {
            Ok(v) => Ok(crate::vm::VmResult::Done(v)),
            Err(e) => Err(e.to_string()),
        }
    } else if let Some(f) = func.as_vm_aware_fn() {
        match f(args, vm) {
            Ok(v) => Ok(crate::vm::VmResult::Done(v)),
            Err(e) => Err(e.into()),
        }
    } else {
        Err(format!("Cannot call {}", func.type_name()))
    };

    // Check for exception set by the function
    if let Some(exc) = vm.current_exception.take() {
        return Err(format!("{}", exc));
    }

    result
}

impl Default for Trampoline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trampoline_done() {
        let mut trampoline = Trampoline::new();
        let result = trampoline.run(Action::Done(Value::int(42)));
        assert!(result.is_done());
        assert_eq!(result.value(), Some(&Value::int(42)));
    }

    #[test]
    fn test_trampoline_yield() {
        let mut trampoline = Trampoline::new();
        let cont = Continuation::done();
        let result = trampoline.run(Action::yield_value(Value::int(1), cont));
        assert!(result.is_suspended());
        assert_eq!(result.value(), Some(&Value::int(1)));
    }

    #[test]
    fn test_trampoline_return_to_done() {
        let mut trampoline = Trampoline::new();
        let cont = Continuation::done();
        let result = trampoline.run(Action::return_value(Value::int(99), cont));
        assert!(result.is_done());
        assert_eq!(result.value(), Some(&Value::int(99)));
    }

    #[test]
    fn test_trampoline_error() {
        let mut trampoline = Trampoline::new();
        let result = trampoline.run(Action::error("test error"));
        match result {
            TrampolineResult::Error(msg) => assert_eq!(msg, "test error"),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_trampoline_max_steps() {
        let config = TrampolineConfig { max_steps: 10 };
        let mut trampoline = Trampoline::with_config(config);

        // Create an infinite loop by returning to a non-done continuation
        // This would loop forever without the max_steps limit
        // For now, we can't create such a loop without VM support
        // So just test that a simple action works within limits
        let result = trampoline.run(Action::Done(Value::NIL));
        assert!(result.is_done());
    }

    #[test]
    fn test_trampoline_step_count() {
        let mut trampoline = Trampoline::new();
        trampoline.run(Action::Done(Value::int(42)));
        assert_eq!(trampoline.step_count(), 1);
    }
}
