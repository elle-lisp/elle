//! Bytecode execution entry points and helpers.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
use std::rc::Rc;

use super::core::VM;

impl VM {
    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    ///
    /// Returns `(SignalBits, ip)` — the signal and the IP at exit.
    pub fn execute_bytecode_from_ip(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> (SignalBits, usize) {
        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();
        let mut current_ip = start_ip;

        loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                current_ip,
            );

            if bits != SIG_OK {
                break (bits, ip);
            }

            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = tail_env;
                current_ip = 0;
            } else {
                break (bits, ip);
            }
        }
    }

    /// Execute bytecode returning SignalBits (for fiber/closure execution).
    /// The result value is stored in `self.fiber.signal`.
    ///
    /// Saves/restores the caller's stack around execution. Handles pending
    /// tail calls in a loop.
    ///
    /// Returns `(SignalBits, ip)` — the signal and the IP at exit.
    pub fn execute_bytecode_saving_stack(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
    ) -> (SignalBits, usize) {
        // Save the caller's stack
        let saved_stack = std::mem::take(&mut self.fiber.stack);

        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();

        let result = loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                0,
            );

            if bits != SIG_OK {
                break (bits, ip);
            }

            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = tail_env;
            } else {
                break (bits, ip);
            }
        };

        // Restore the caller's stack
        self.fiber.stack = saved_stack;

        result
    }

    /// Execute closure bytecode without copying.
    ///
    /// Like `execute_bytecode` but takes `&Rc` references directly,
    /// avoiding the `.to_vec()` copies that `execute_bytecode` performs.
    /// Used by JIT trampolines where the closure already owns Rc'd data.
    ///
    /// Handles the tail-call loop and translates SignalBits to
    /// `Result<Value, String>`.
    pub fn execute_closure_bytecode(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
    ) -> Result<Value, String> {
        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();

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
                    SIG_OK | SIG_HALT => {
                        let (_, value) = self.fiber.signal.take().unwrap();
                        Ok(value)
                    }
                    SIG_YIELD => Err("Unexpected yield outside coroutine context".to_string()),
                    SIG_ERROR => {
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
