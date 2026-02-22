//! Bytecode execution entry points and helpers.

use crate::value::{SignalBits, Value, SIG_OK};
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
}
