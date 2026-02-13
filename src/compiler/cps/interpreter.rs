//! CPS expression interpreter
//!
//! Evaluates CpsExpr trees and produces Actions for the trampoline.
//! This interpreter is used for coroutine execution when the closure
//! has a yielding effect.

use super::{Action, Continuation, CpsExpr};
use crate::value::{SymbolId, Value};
use crate::vm::VM;
use std::collections::HashMap;
use std::rc::Rc;

/// Update a continuation's "next" field to chain with another continuation
/// This is used when a yielding construct (like while) is inside a sequence
fn update_continuation_next(
    cont: Rc<Continuation>,
    new_next: Rc<Continuation>,
) -> Rc<Continuation> {
    match cont.as_ref() {
        Continuation::Done => {
            // If the continuation is done, just use the new next
            new_next
        }
        Continuation::CpsWhile {
            cond,
            body,
            next: _,
        } => {
            // Update the while loop's next to chain with new_next
            Rc::new(Continuation::CpsWhile {
                cond: cond.clone(),
                body: body.clone(),
                next: new_next,
            })
        }
        Continuation::CpsWhileBody {
            body_cont,
            cond,
            body,
            next: _,
        } => {
            // Update the while body's next to chain with new_next
            Rc::new(Continuation::CpsWhileBody {
                body_cont: body_cont.clone(),
                cond: cond.clone(),
                body: body.clone(),
                next: new_next,
            })
        }
        Continuation::CpsSequence { remaining, next: _ } => {
            // Update the sequence's next to chain with new_next
            Rc::new(Continuation::CpsSequence {
                remaining: remaining.clone(),
                next: new_next,
            })
        }
        _ => {
            // For other continuation types, wrap in CpsSequenceAfterYield
            Rc::new(Continuation::CpsSequenceAfterYield {
                yield_cont: cont,
                remaining_cont: new_next,
            })
        }
    }
}

/// CPS interpreter state
pub struct CpsInterpreter<'a> {
    vm: &'a mut VM,
    /// Local variable bindings
    locals: HashMap<SymbolId, Value>,
    /// Closure environment
    env: Rc<Vec<Value>>,
}

impl<'a> CpsInterpreter<'a> {
    /// Create a new CPS interpreter
    pub fn new(vm: &'a mut VM, env: Rc<Vec<Value>>) -> Self {
        Self {
            vm,
            locals: HashMap::new(),
            env,
        }
    }

    /// Get the environment (for trampoline access)
    pub fn env(&self) -> &Rc<Vec<Value>> {
        &self.env
    }

    /// Evaluate a CPS expression and return an Action
    pub fn eval(&mut self, expr: &CpsExpr) -> Result<Action, String> {
        match expr {
            CpsExpr::Literal(v) => Ok(Action::done(v.clone())),

            CpsExpr::Var { sym, depth, index } => {
                // Check locals first
                if let Some(val) = self.locals.get(sym) {
                    return Ok(Action::done(val.clone()));
                }
                // Then check closure environment
                if *depth == 0 && *index < self.env.len() {
                    let val = self.env[*index].clone();
                    // Unwrap LocalCell if needed
                    let unwrapped = self.unwrap_local_cell(val);
                    return Ok(Action::done(unwrapped));
                }
                Err(format!("Variable not found: {:?}", sym))
            }

            CpsExpr::GlobalVar(sym) => {
                // Check locals first (for variables defined in CPS context via let/define)
                if let Some(val) = self.locals.get(sym) {
                    return Ok(Action::done(val.clone()));
                }
                // Then check globals
                if let Some(val) = self.vm.globals.get(&sym.0) {
                    Ok(Action::done(val.clone()))
                } else {
                    Err(format!("Undefined global: {:?}", sym))
                }
            }

            CpsExpr::Yield {
                value,
                continuation,
            } => {
                // Evaluate the value expression
                let val_action = self.eval(value)?;
                match val_action {
                    Action::Done(val) => {
                        // Yield the value with the continuation for resumption
                        Ok(Action::yield_value(val, continuation.clone()))
                    }
                    other => Ok(other), // Propagate other actions
                }
            }

            CpsExpr::Pure { expr, continuation } => {
                // Evaluate pure expression using VM
                let val = self.eval_pure_expr(expr)?;
                // Apply continuation
                Ok(Action::return_value(val, continuation.clone()))
            }

            CpsExpr::PureCall {
                func,
                args,
                continuation,
            } => {
                // Evaluate function and args
                let func_val = self.eval_to_value(func)?;
                let arg_vals: Result<Vec<Value>, String> =
                    args.iter().map(|a| self.eval_to_value(a)).collect();
                let arg_vals = arg_vals?;

                // Call the function
                let result = self.call_value(&func_val, &arg_vals)?;

                // Apply continuation to result
                Ok(Action::return_value(result, continuation.clone()))
            }

            CpsExpr::CpsCall {
                func,
                args,
                continuation,
            } => {
                // Evaluate function and args
                let func_val = self.eval_to_value(func)?;
                let arg_vals: Result<Vec<Value>, String> =
                    args.iter().map(|a| self.eval_to_value(a)).collect();
                let arg_vals = arg_vals?;

                // Return a Call action - the trampoline will handle it
                Ok(Action::call(func_val, arg_vals, continuation.clone()))
            }

            CpsExpr::Let { var, init, body } => {
                // Evaluate initializer
                let init_val = self.eval_to_value(init)?;

                // Bind variable
                self.locals.insert(*var, init_val);

                // Evaluate body
                self.eval(body)
            }

            CpsExpr::Sequence {
                exprs,
                continuation,
            } => {
                if exprs.is_empty() {
                    return Ok(Action::return_value(Value::Nil, continuation.clone()));
                }

                // Evaluate expressions one by one, capturing remaining on yield
                for (i, expr) in exprs.iter().enumerate() {
                    let action = self.eval(expr)?;

                    match action {
                        Action::Done(_) => {
                            // Continue to next expression
                            // (value is discarded unless this is the last expression)
                            if i == exprs.len() - 1 {
                                if let Action::Done(val) = action {
                                    return Ok(Action::return_value(val, continuation.clone()));
                                }
                            }
                        }
                        Action::Yield {
                            value,
                            continuation: yield_cont,
                        } => {
                            // Yield happened - capture remaining expressions
                            let remaining = exprs[i + 1..].to_vec();

                            // Create a continuation that will:
                            // 1. First, continue with yield_cont (e.g., continue a while loop)
                            // 2. Then, evaluate remaining expressions
                            // 3. Finally, continue with the outer continuation
                            let resume_cont = if remaining.is_empty() {
                                // No more expressions - use the yield's continuation
                                yield_cont
                            } else {
                                // Create a continuation that evaluates remaining expressions
                                let remaining_cont = Rc::new(Continuation::CpsSequence {
                                    remaining,
                                    next: continuation.clone(),
                                });

                                // Update yield_cont's "next" to include remaining expressions
                                // This ensures that when the yielding construct completes,
                                // it continues with the remaining expressions
                                update_continuation_next(yield_cont, remaining_cont)
                            };

                            return Ok(Action::yield_value(value, resume_cont));
                        }
                        other => return Ok(other), // Propagate errors, calls, etc.
                    }
                }

                // Should not reach here
                Ok(Action::return_value(Value::Nil, continuation.clone()))
            }

            CpsExpr::If {
                cond,
                then_branch,
                else_branch,
                continuation,
            } => {
                // Evaluate condition
                let cond_val = self.eval_to_value(cond)?;

                // Choose branch
                let branch = if cond_val.is_truthy() {
                    then_branch
                } else {
                    else_branch
                };

                // Evaluate chosen branch
                let result = self.eval(branch)?;
                match result {
                    Action::Done(val) => Ok(Action::return_value(val, continuation.clone())),
                    other => Ok(other),
                }
            }

            CpsExpr::While {
                cond,
                body,
                continuation,
            } => {
                loop {
                    // Evaluate condition
                    let cond_val = self.eval_to_value(cond)?;
                    if !cond_val.is_truthy() {
                        break;
                    }

                    // Evaluate body
                    let body_action = self.eval(body)?;

                    // If body yields, capture the while loop state
                    if let Action::Yield {
                        value,
                        continuation: body_cont,
                    } = body_action
                    {
                        // The body_cont is the continuation for the rest of the body
                        // We need to chain: body_cont -> loop again -> outer continuation
                        // Create a continuation that will continue the while loop
                        let while_cont = Rc::new(Continuation::CpsWhile {
                            cond: cond.clone(),
                            body: body.clone(),
                            next: continuation.clone(),
                        });

                        // Chain the body continuation with the while continuation
                        let chained_cont = if body_cont.is_done() {
                            // Body is done after yield, continue with while loop
                            while_cont
                        } else {
                            // Body has more to do, then continue with while loop
                            // Use CpsWhileBody to handle this
                            Rc::new(Continuation::CpsWhileBody {
                                body_cont,
                                cond: cond.clone(),
                                body: body.clone(),
                                next: continuation.clone(),
                            })
                        };

                        return Ok(Action::yield_value(value, chained_cont));
                    }
                }
                Ok(Action::return_value(Value::Nil, continuation.clone()))
            }

            CpsExpr::Lambda {
                params: _,
                body: _,
                captures: _,
            } => {
                // Create a closure value
                // For CPS lambdas, we need to store the CPS body
                // For now, return a placeholder - full implementation needs
                // to create a proper CpsLambda value type or use existing Closure
                Err("CPS Lambda creation not yet implemented".to_string())
            }

            CpsExpr::And {
                exprs,
                continuation,
            } => {
                let mut result = Value::Bool(true);
                for expr in exprs {
                    let action = self.eval(expr)?;
                    match action {
                        Action::Done(val) | Action::Return { value: val, .. } => {
                            if !val.is_truthy() {
                                return Ok(Action::return_value(val, continuation.clone()));
                            }
                            result = val;
                        }
                        Action::Yield { .. } => return Ok(action),
                        Action::Error(e) => return Err(e),
                        _ => return Err("Unexpected action in And expression".to_string()),
                    }
                }
                Ok(Action::return_value(result, continuation.clone()))
            }

            CpsExpr::Or {
                exprs,
                continuation,
            } => {
                for expr in exprs {
                    let action = self.eval(expr)?;
                    match action {
                        Action::Done(val) | Action::Return { value: val, .. } => {
                            if val.is_truthy() {
                                return Ok(Action::return_value(val, continuation.clone()));
                            }
                        }
                        Action::Yield { .. } => return Ok(action),
                        Action::Error(e) => return Err(e),
                        _ => return Err("Unexpected action in Or expression".to_string()),
                    }
                }
                Ok(Action::return_value(
                    Value::Bool(false),
                    continuation.clone(),
                ))
            }

            CpsExpr::Cond {
                clauses,
                else_body,
                continuation,
            } => {
                for (cond, body) in clauses {
                    let cond_val = self.eval_to_value(cond)?;
                    if cond_val.is_truthy() {
                        let result = self.eval(body)?;
                        match result {
                            Action::Done(val) => {
                                return Ok(Action::return_value(val, continuation.clone()));
                            }
                            other => return Ok(other),
                        }
                    }
                }

                // No clause matched - evaluate else body or return nil
                if let Some(else_expr) = else_body {
                    let result = self.eval(else_expr)?;
                    match result {
                        Action::Done(val) => Ok(Action::return_value(val, continuation.clone())),
                        other => Ok(other),
                    }
                } else {
                    Ok(Action::return_value(Value::Nil, continuation.clone()))
                }
            }

            CpsExpr::For {
                var,
                iter,
                body,
                continuation,
            } => {
                // Evaluate iterator
                let iter_val = self.eval_to_value(iter)?;

                // Convert to iterable
                let items = self.value_to_list(&iter_val)?;

                for item in items {
                    self.locals.insert(*var, item);
                    let body_action = self.eval(body)?;

                    if let Action::Yield { .. } = body_action {
                        return Ok(body_action);
                    }
                }

                Ok(Action::return_value(Value::Nil, continuation.clone()))
            }

            CpsExpr::Return(expr) => {
                let val = self.eval_to_value(expr)?;
                Ok(Action::done(val))
            }
        }
    }

    /// Evaluate a CPS expression to a Value (blocking on yields)
    fn eval_to_value(&mut self, expr: &CpsExpr) -> Result<Value, String> {
        match self.eval(expr)? {
            Action::Done(val) => Ok(val),
            Action::Return { value, .. } => Ok(value),
            Action::Yield { .. } => Err("Unexpected yield in pure context".to_string()),
            Action::Call { .. } => Err("Unexpected call in value context".to_string()),
            Action::TailCall { .. } => Err("Unexpected tail call in value context".to_string()),
            Action::Error(e) => Err(e),
        }
    }

    /// Evaluate a pure Expr using direct interpretation
    fn eval_pure_expr(&mut self, expr: &crate::compiler::ast::Expr) -> Result<Value, String> {
        use crate::compiler::ast::Expr;

        match expr {
            Expr::Literal(v) => Ok(v.clone()),

            Expr::Var(sym, depth, index) => {
                // Check locals first (for variables defined in CPS context)
                if let Some(val) = self.locals.get(sym) {
                    return Ok(val.clone());
                }
                // Check globals (for variables defined via define in coroutines)
                if let Some(val) = self.vm.globals.get(&sym.0) {
                    return Ok(val.clone());
                }
                // Then check closure environment
                if *depth == 0 && *index < self.env.len() {
                    let val = self.env[*index].clone();
                    Ok(self.unwrap_local_cell(val))
                } else {
                    Err(format!(
                        "Variable not found at depth={}, index={}",
                        depth, index
                    ))
                }
            }

            Expr::GlobalVar(sym) => {
                // Check locals first (for variables defined in CPS context via let/define)
                if let Some(val) = self.locals.get(sym) {
                    return Ok(val.clone());
                }
                // Then check globals
                if let Some(val) = self.vm.globals.get(&sym.0) {
                    Ok(val.clone())
                } else {
                    Err(format!("Undefined global: {:?}", sym))
                }
            }

            Expr::If { cond, then, else_ } => {
                let cond_val = self.eval_pure_expr(cond)?;
                if cond_val.is_truthy() {
                    self.eval_pure_expr(then)
                } else {
                    self.eval_pure_expr(else_)
                }
            }

            Expr::Begin(exprs) => {
                let mut result = Value::Nil;
                for e in exprs {
                    result = self.eval_pure_expr(e)?;
                }
                Ok(result)
            }

            Expr::Block(exprs) => {
                let mut result = Value::Nil;
                for e in exprs {
                    result = self.eval_pure_expr(e)?;
                }
                Ok(result)
            }

            Expr::Call { func, args, .. } => {
                let func_val = self.eval_pure_expr(func)?;
                let arg_vals: Result<Vec<Value>, String> =
                    args.iter().map(|a| self.eval_pure_expr(a)).collect();
                let arg_vals = arg_vals?;
                self.call_value(&func_val, &arg_vals)
            }

            Expr::And(exprs) => {
                let mut result = Value::Bool(true);
                for e in exprs {
                    let val = self.eval_pure_expr(e)?;
                    if !val.is_truthy() {
                        return Ok(val);
                    }
                    result = val;
                }
                Ok(result)
            }

            Expr::Or(exprs) => {
                for e in exprs {
                    let val = self.eval_pure_expr(e)?;
                    if val.is_truthy() {
                        return Ok(val);
                    }
                }
                Ok(Value::Bool(false))
            }

            Expr::Cond { clauses, else_body } => {
                for (cond, body) in clauses {
                    let cond_val = self.eval_pure_expr(cond)?;
                    if cond_val.is_truthy() {
                        return self.eval_pure_expr(body);
                    }
                }
                if let Some(else_expr) = else_body {
                    self.eval_pure_expr(else_expr)
                } else {
                    Ok(Value::Nil)
                }
            }

            Expr::Let { bindings, body } => {
                // Evaluate bindings and add to locals
                for (var, init) in bindings {
                    let val = self.eval_pure_expr(init)?;
                    self.locals.insert(*var, val);
                }
                self.eval_pure_expr(body)
            }

            Expr::Define { name, value } => {
                // Evaluate the value and store
                let val = self.eval_pure_expr(value)?;
                // Inside a coroutine/function body, define creates a local binding
                // Store in locals for the CPS interpreter
                self.locals.insert(*name, val.clone());
                // Also store in globals for compatibility with global lookups
                self.vm.set_global(name.0, val.clone());
                Ok(val)
            }

            Expr::Set {
                var,
                depth,
                index,
                value,
            } => {
                // Evaluate the value and update the target
                let val = self.eval_pure_expr(value)?;
                // Check locals first
                if self.locals.contains_key(var) {
                    self.locals.insert(*var, val.clone());
                } else if self.vm.globals.contains_key(&var.0) {
                    // Update in globals (for variables defined via define in coroutines)
                    self.vm.set_global(var.0, val.clone());
                } else if *depth == 0 && *index < self.env.len() {
                    // Update in environment (if it's a LocalCell)
                    let env_val = &self.env[*index];
                    if let Value::LocalCell(cell) = env_val {
                        **cell.borrow_mut() = val.clone();
                    }
                }
                Ok(val)
            }

            // For other expressions, return an error for now
            // These would need more complex handling
            _ => Err(format!(
                "Pure expression type not yet supported: {:?}",
                expr
            )),
        }
    }

    /// Call a value as a function with the given arguments
    fn call_value(&mut self, func: &Value, args: &[Value]) -> Result<Value, String> {
        match func {
            Value::Closure(closure) => {
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
                    let empty_cell = Value::LocalCell(std::rc::Rc::new(std::cell::RefCell::new(
                        Box::new(Value::Nil),
                    )));
                    new_env.push(empty_cell);
                }

                let env_rc = std::rc::Rc::new(new_env);

                // Execute
                self.vm
                    .execute_bytecode(&closure.bytecode, &closure.constants, Some(&env_rc))
            }

            Value::NativeFn(f) => f(args),

            Value::VmAwareFn(f) => f(args, self.vm),

            _ => Err(format!("Cannot call {}", func.type_name())),
        }
    }

    /// Unwrap LocalCell if present
    fn unwrap_local_cell(&self, val: Value) -> Value {
        match val {
            Value::LocalCell(cell_rc) => {
                let borrowed = cell_rc.borrow();
                (**borrowed).clone()
            }
            other => other,
        }
    }

    /// Convert a Value to a list of Values for iteration
    fn value_to_list(&self, val: &Value) -> Result<Vec<Value>, String> {
        match val {
            Value::Nil => Ok(vec![]),
            Value::Cons(_) => {
                let mut result = vec![];
                let mut current = val.clone();
                while let Value::Cons(cons) = current {
                    result.push(cons.first.clone());
                    current = cons.rest.clone();
                }
                Ok(result)
            }
            Value::Vector(v) => Ok((**v).clone()),
            _ => Err(format!("Cannot iterate over {}", val.type_name())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::cps::Continuation;

    #[test]
    fn test_eval_literal() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::Literal(Value::Int(42));
        let result = interp.eval(&expr).unwrap();

        assert!(result.is_done());
        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 42),
            _ => panic!("Expected Done(Int(42))"),
        }
    }

    #[test]
    fn test_eval_global_var() {
        let mut vm = VM::new();
        vm.set_global(1, Value::Int(100));

        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::GlobalVar(SymbolId(1));
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 100),
            _ => panic!("Expected Done(Int(100))"),
        }
    }

    #[test]
    fn test_eval_yield() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::Yield {
            value: Box::new(CpsExpr::Literal(Value::Int(42))),
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        assert!(result.is_yield());
        match result {
            Action::Yield { value, .. } => assert_eq!(value, Value::Int(42)),
            _ => panic!("Expected Yield"),
        }
    }

    #[test]
    fn test_eval_let() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let var = SymbolId(1);
        let expr = CpsExpr::Let {
            var,
            init: Box::new(CpsExpr::Literal(Value::Int(10))),
            body: Box::new(CpsExpr::Literal(Value::Int(20))),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 20),
            _ => panic!("Expected Done(Int(20))"),
        }
    }

    #[test]
    fn test_eval_if_true() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::If {
            cond: Box::new(CpsExpr::Literal(Value::Bool(true))),
            then_branch: Box::new(CpsExpr::Literal(Value::Int(1))),
            else_branch: Box::new(CpsExpr::Literal(Value::Int(2))),
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 1),
            _ => panic!("Expected Done(Int(1))"),
        }
    }

    #[test]
    fn test_eval_if_false() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::If {
            cond: Box::new(CpsExpr::Literal(Value::Bool(false))),
            then_branch: Box::new(CpsExpr::Literal(Value::Int(1))),
            else_branch: Box::new(CpsExpr::Literal(Value::Int(2))),
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 2),
            _ => panic!("Expected Done(Int(2))"),
        }
    }

    #[test]
    fn test_eval_sequence() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::Sequence {
            exprs: vec![
                CpsExpr::Literal(Value::Int(1)),
                CpsExpr::Literal(Value::Int(2)),
                CpsExpr::Literal(Value::Int(3)),
            ],
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 3),
            _ => panic!("Expected Done(Int(3))"),
        }
    }

    #[test]
    fn test_eval_and_short_circuit() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::And {
            exprs: vec![
                CpsExpr::Literal(Value::Bool(true)),
                CpsExpr::Literal(Value::Bool(false)),
                CpsExpr::Literal(Value::Bool(true)),
            ],
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Bool(b)) => assert!(!b),
            _ => panic!("Expected Done(Bool(false))"),
        }
    }

    #[test]
    fn test_eval_or_short_circuit() {
        let mut vm = VM::new();
        let env = Rc::new(vec![]);
        let mut interp = CpsInterpreter::new(&mut vm, env);

        let expr = CpsExpr::Or {
            exprs: vec![
                CpsExpr::Literal(Value::Bool(false)),
                CpsExpr::Literal(Value::Int(42)),
                CpsExpr::Literal(Value::Bool(false)),
            ],
            continuation: Continuation::done(),
        };
        let result = interp.eval(&expr).unwrap();

        match result {
            Action::Done(Value::Int(n)) => assert_eq!(n, 42),
            _ => panic!("Expected Done(Int(42))"),
        }
    }
}
