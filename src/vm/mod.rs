pub mod arithmetic;
pub mod call;
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod dispatch;
pub mod execute;
pub mod fiber;
pub mod literals;
pub mod scope;
pub mod signal;
pub mod stack;
pub mod types;
pub mod variables;

pub use crate::value::fiber::CallFrame;
pub use core::VM;

use crate::compiler::bytecode::Bytecode;
use crate::value::{error_val, Value, SIG_ERROR, SIG_OK, SIG_YIELD};
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        self.location_map = bytecode.location_map.clone();
        self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)
    }

    /// Check arity and set error signal if mismatch.
    /// Returns true if arity is OK, false if there's a mismatch.
    pub(crate) fn check_arity(&mut self, arity: &crate::value::Arity, arg_count: usize) -> bool {
        let mismatch = match arity {
            crate::value::Arity::Exact(n) if arg_count != *n => {
                Some(format!("expected {} arguments, got {}", n, arg_count))
            }
            crate::value::Arity::AtLeast(n) if arg_count < *n => Some(format!(
                "expected at least {} arguments, got {}",
                n, arg_count
            )),
            crate::value::Arity::Range(min, max) if arg_count < *min || arg_count > *max => Some(
                format!("expected {}-{} arguments, got {}", min, max, arg_count),
            ),
            _ => None,
        };

        if let Some(msg) = mismatch {
            self.fiber.signal = Some((SIG_ERROR, error_val("arity-error", msg)));
            return false;
        }
        true
    }

    /// Execute raw bytecode with optional closure environment.
    ///
    /// Translation boundary: internally uses SignalBits, externally
    /// returns `Result<Value, String>`. Wraps slices in `Rc` once at
    /// the boundary.
    pub fn execute_bytecode(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        let empty_env = Rc::new(vec![]);
        let mut current_bytecode = Rc::new(bytecode.to_vec());
        let mut current_constants = Rc::new(constants.to_vec());
        let mut current_env = closure_env.cloned().unwrap_or(empty_env);

        loop {
            let (bits, _ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                0,
            );

            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = tail_env;
            } else {
                return match bits {
                    SIG_OK => {
                        let (_, value) = self.fiber.signal.take().unwrap();
                        Ok(value)
                    }
                    SIG_YIELD => Err("Unexpected yield outside coroutine context".to_string()),
                    SIG_ERROR => {
                        // Extract the error from fiber.signal
                        let (_, err_value) =
                            self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                        Err(crate::value::format_error(err_value))
                    }
                    _ => {
                        panic!("VM bug: Unexpected signal: {}", bits);
                    }
                };
            }
        }
    }
}
