pub mod arithmetic;
pub mod call;
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod dispatch;
pub mod execute;
pub mod literals;
pub mod scope;
pub mod stack;
pub mod types;
pub mod variables;

pub use crate::value::condition::{exception_parent, is_exception_subclass};
pub use core::{CallFrame, VmResult, VM};

use crate::compiler::bytecode::Bytecode;
use crate::value::Value;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        // Set the location map for error reporting
        self.location_map = bytecode.location_map.clone();

        let result = self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)?;
        // Check if an exception escaped all handlers at the top level
        if let Some(exc) = &self.current_exception {
            // Use the exception's message for the error
            Err(format!("{}", exc))
        } else {
            Ok(result)
        }
    }

    /// Check arity and set exception if mismatch.
    /// Returns true if arity is OK, false if there's a mismatch (exception is set).
    pub(crate) fn check_arity(&mut self, arity: &crate::value::Arity, arg_count: usize) -> bool {
        match arity {
            crate::value::Arity::Exact(n) => {
                if arg_count != *n {
                    let msg = format!("expected {} arguments, got {}", n, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*n as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
            crate::value::Arity::AtLeast(n) => {
                if arg_count < *n {
                    let msg = format!("expected at least {} arguments, got {}", n, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*n as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
            crate::value::Arity::Range(min, max) => {
                if arg_count < *min || arg_count > *max {
                    let msg = format!("expected {}-{} arguments, got {}", min, max, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*min as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
        }
        true
    }

    /// Execute raw bytecode with optional closure environment.
    ///
    /// This is used internally for closure execution and by spawn/join primitives
    /// to execute closures in spawned threads.
    ///
    /// This function handles tail calls without recursion by using a loop-based approach.
    pub fn execute_bytecode(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&std::rc::Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        // Outer loop to handle tail calls without recursion
        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();

        loop {
            let result = self.execute_bytecode_inner(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
            )?;

            // Check if there's a pending tail call
            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
                // Continue the loop to execute the tail call
            } else {
                // No pending tail call, return the result
                return match result {
                    VmResult::Done(v) => Ok(v),
                    VmResult::Yielded { .. } => {
                        // Yield should be handled by coroutine_resume, not here
                        Err("Unexpected yield outside coroutine context".to_string())
                    }
                };
            }
        }
    }
}
