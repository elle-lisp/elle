pub mod arithmetic;
pub mod call;
pub mod cell;
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod dispatch;
pub mod eval;
pub mod execute;
pub mod fiber;
pub mod literals;
pub mod parameters;
pub mod signal;
pub mod stack;
pub mod types;
pub mod variables;

pub use crate::value::fiber::CallFrame;
pub use core::VM;

use crate::compiler::bytecode::Bytecode;
use crate::io::backend::SyncBackend;
use crate::io::request::IoRequest;
use crate::symbol::SymbolTable;
use crate::value::{error_val, Value, SIG_ERROR, SIG_HALT, SIG_IO, SIG_OK, SIG_YIELD};
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
        self.error_loc = None;

        let empty_env = Rc::new(vec![]);
        let mut current_bytecode = Rc::new(bytecode.to_vec());
        let mut current_constants = Rc::new(constants.to_vec());
        let mut current_env = closure_env.cloned().unwrap_or(empty_env);
        let mut current_location_map = Rc::new(self.location_map.clone());

        loop {
            let (bits, _ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                0,
                &current_location_map,
            );

            if let Some(tail) = self.pending_tail_call.take() {
                current_bytecode = tail.bytecode;
                current_constants = tail.constants;
                current_env = tail.env;
                current_location_map = tail.location_map;
            } else {
                return match bits {
                    SIG_OK | SIG_HALT => {
                        let (_, value) = self.fiber.signal.take().unwrap();
                        Ok(value)
                    }
                    SIG_YIELD => Err("Unexpected yield outside coroutine context".to_string()),
                    SIG_IO => {
                        self.fiber.signal.take();
                        Err("Unexpected SIG_IO outside scheduler context".to_string())
                    }
                    SIG_ERROR => {
                        // Extract the error from fiber.signal
                        let (_, err_value) =
                            self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                        Err(self.format_error_with_location(err_value))
                    }
                    _ => {
                        panic!("VM bug: Unexpected signal: {}", bits);
                    }
                };
            }
        }
    }

    /// Execute bytecode with inline SIG_IO handling.
    ///
    /// Used for user-facing execution (files, REPL). Not used for
    /// internal compilation (prelude, stdlib, macro expansion).
    ///
    /// Unlike `execute`, which errors on SIG_IO, this method handles
    /// I/O requests inline: when a stream primitive yields SIG_IO,
    /// the request is executed synchronously via SyncBackend and the
    /// result is pushed back onto the stack. Execution then resumes
    /// from where it left off.
    ///
    /// This avoids wrapping user code in a fiber, which would add an
    /// extra nesting level and break code that manages its own fibers
    /// (e.g., process schedulers, coroutine examples).
    ///
    /// If `*scheduler*` is not yet defined (stdlib hasn't loaded),
    /// falls back to `vm.execute` (direct execution without I/O).
    pub fn execute_scheduled(
        &mut self,
        bytecode: &Bytecode,
        symbols: &SymbolTable,
    ) -> Result<Value, String> {
        // Check if *scheduler* exists as a gate: if stdlib hasn't loaded
        // yet, fall back to direct execution (no I/O support needed).
        let has_scheduler = symbols
            .get("*scheduler*")
            .and_then(|id| self.get_global(id.0))
            .is_some();

        if !has_scheduler {
            return self.execute(bytecode);
        }

        // Execute with inline SIG_IO handling.
        self.location_map = bytecode.location_map.clone();
        self.error_loc = None;

        let empty_env = Rc::new(vec![]);
        let mut current_bytecode = Rc::new(bytecode.instructions.to_vec());
        let mut current_constants = Rc::new(bytecode.constants.to_vec());
        let mut current_env = empty_env;
        let mut current_location_map = Rc::new(self.location_map.clone());
        let mut current_ip = 0usize;
        let mut backend: Option<SyncBackend> = None;

        loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                current_ip,
                &current_location_map,
            );

            if let Some(tail) = self.pending_tail_call.take() {
                current_bytecode = tail.bytecode;
                current_constants = tail.constants;
                current_env = tail.env;
                current_location_map = tail.location_map;
                current_ip = 0;
                continue;
            }

            match bits {
                SIG_OK | SIG_HALT => {
                    let (_, value) = self.fiber.signal.take().unwrap();
                    return Ok(value);
                }
                SIG_IO => {
                    // Extract the IoRequest from the signal, execute it
                    // with the backend, push the result, and resume.
                    let (_, request_val) = self.fiber.signal.take().unwrap();
                    let backend = backend.get_or_insert_with(SyncBackend::new);
                    match request_val.as_external::<IoRequest>() {
                        Some(req) => {
                            let (result_bits, result_val) = backend.execute(req);
                            if result_bits == SIG_ERROR {
                                return Err(self.format_error_with_location(result_val));
                            }
                            self.fiber.stack.push(result_val);
                        }
                        None => {
                            return Err(format!(
                                "SIG_IO with non-IoRequest value: {}",
                                request_val.type_name()
                            ));
                        }
                    }
                    current_ip = ip;
                }
                SIG_YIELD => {
                    return Err("Unexpected yield outside coroutine context".to_string());
                }
                SIG_ERROR => {
                    let (_, err_value) =
                        self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                    return Err(self.format_error_with_location(err_value));
                }
                _ => {
                    panic!("VM bug: Unexpected signal: {}", bits);
                }
            }
        }
    }
}
