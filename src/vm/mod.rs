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
use crate::port::Port;
use crate::symbol::SymbolTable;
use crate::value::{error_val, Value, SIG_ERROR, SIG_HALT, SIG_IO, SIG_YIELD};
use std::rc::Rc;
use std::time::{Duration, Instant};

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
                return if bits.is_ok() || bits == SIG_HALT {
                    let (_, value) = self.fiber.signal.take().unwrap();
                    Ok(value)
                } else if bits.contains(SIG_IO) {
                    self.fiber.signal.take();
                    Err("Unexpected SIG_IO outside scheduler context".to_string())
                } else if bits.contains(SIG_YIELD) {
                    Err("Unexpected yield outside coroutine context".to_string())
                } else if bits.contains(SIG_ERROR) {
                    // Extract the error from fiber.signal
                    let (_, err_value) =
                        self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                    Err(self.format_error_with_location(err_value))
                } else {
                    panic!("VM bug: Unexpected signal: {}", bits);
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
        //
        // When a primitive yields SIG_IO, the VM saves the entire call
        // stack into `fiber.suspended` (a Vec<SuspendedFrame>). We
        // execute the I/O synchronously, then replay the suspended
        // frames via `resume_suspended` — the same mechanism used by
        // fiber resume. This correctly restores locals, call frames,
        // and stack state at every nesting level.
        self.location_map = bytecode.location_map.clone();
        self.error_loc = None;

        let mut backend: Option<SyncBackend> = None;

        // Initial execution.
        let mut bits = {
            let bc = Rc::new(bytecode.instructions.to_vec());
            let cs = Rc::new(bytecode.constants.to_vec());
            let env = Rc::new(vec![]);
            let lm = Rc::new(self.location_map.clone());
            self.execute_bytecode_inner_impl(&bc, &cs, &env, 0, &lm).0
        };

        loop {
            if let Some(tail) = self.pending_tail_call.take() {
                bits = self
                    .execute_bytecode_inner_impl(
                        &tail.bytecode,
                        &tail.constants,
                        &tail.env,
                        0,
                        &tail.location_map,
                    )
                    .0;
                continue;
            }

            if bits.is_ok() || bits == SIG_HALT {
                let (_, value) = self.fiber.signal.take().unwrap();
                return Ok(value);
            } else if bits.contains(SIG_IO) {
                // Extract the IoRequest, execute I/O, then resume the
                // suspended frame chain with the result.
                let (_, request_val) = self.fiber.signal.take().unwrap();
                let backend = backend.get_or_insert_with(SyncBackend::new);
                let io_result = match request_val.as_external::<IoRequest>() {
                    Some(req) => {
                        // Resolve effective timeout: per-call overrides port-level.
                        let effective_timeout = req.timeout.or_else(|| {
                            req.port
                                .as_external::<Port>()
                                .and_then(|p| p.timeout_ms())
                                .map(Duration::from_millis)
                        });
                        let deadline = effective_timeout.map(|d| Instant::now() + d);

                        let (result_bits, result_val) = backend.execute(req);

                        // Post-hoc deadline check (sync backend blocks).
                        if let Some(dl) = deadline {
                            if Instant::now() > dl {
                                return Err(self.format_error_with_location(error_val(
                                    "timeout",
                                    "I/O operation timed out",
                                )));
                            }
                        }

                        if result_bits.contains(SIG_ERROR) {
                            return Err(self.format_error_with_location(result_val));
                        }
                        result_val
                    }
                    None => {
                        return Err(format!(
                            "SIG_IO with non-IoRequest value: {}",
                            request_val.type_name()
                        ));
                    }
                };

                // Resume the suspended frame chain with the I/O result.
                // This restores locals and call frames at every level.
                let frames = self.fiber.suspended.take().unwrap_or_default();
                bits = self.resume_suspended(frames, io_result);
            } else if bits.contains(SIG_YIELD) {
                return Err("Unexpected yield outside coroutine context".to_string());
            } else if bits.contains(SIG_ERROR) {
                let (_, err_value) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                return Err(self.format_error_with_location(err_value));
            } else {
                panic!("VM bug: Unexpected signal: {}", bits);
            }
        }
    }
}
