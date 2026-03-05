//! Bytecode execution entry points and helpers.

use crate::error::LocationMap;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
use std::rc::Rc;

use super::core::VM;

/// Result of `execute_bytecode_saving_stack`.
///
/// Contains the signal, IP, and the active bytecode/constants/env at exit.
/// When a tail call occurs before a signal, the active context differs from
/// the original closure — callers that create `SuspendedFrame`s must use
/// these fields, not the original closure's bytecode/constants.
pub struct ExecResult {
    pub bits: SignalBits,
    pub ip: usize,
    pub bytecode: Rc<Vec<u8>>,
    pub constants: Rc<Vec<Value>>,
    pub env: Rc<Vec<Value>>,
    pub location_map: Rc<LocationMap>,
}

impl VM {
    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    ///
    /// Returns `ExecResult` containing the signal, IP, and the active
    /// bytecode/constants/env at exit. The active context may differ from
    /// the input if a tail call occurred before the signal.
    pub fn execute_bytecode_from_ip(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> ExecResult {
        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();
        let mut current_location_map = location_map.clone();
        let mut current_ip = start_ip;

        loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                current_ip,
                &current_location_map,
            );

            if bits != SIG_OK {
                break ExecResult {
                    bits,
                    ip,
                    bytecode: current_bytecode,
                    constants: current_constants,
                    env: current_env,
                    location_map: current_location_map,
                };
            }

            if let Some(tail) = self.pending_tail_call.take() {
                current_bytecode = tail.bytecode;
                current_constants = tail.constants;
                current_env = tail.env;
                current_location_map = tail.location_map;
                current_ip = 0;
            } else {
                break ExecResult {
                    bits,
                    ip,
                    bytecode: current_bytecode,
                    constants: current_constants,
                    env: current_env,
                    location_map: current_location_map,
                };
            }
        }
    }

    /// Execute bytecode returning SignalBits (for fiber/closure execution).
    /// The result value is stored in `self.fiber.signal`.
    ///
    /// Saves/restores the caller's stack and the active allocator pointer
    /// around execution. Handles pending tail calls in a loop.
    ///
    /// Returns `ExecResult` containing the signal, IP, and the active
    /// bytecode/constants/env at exit. The active context may differ from
    /// the input if a tail call occurred before the signal — callers that
    /// create `SuspendedFrame`s must use the returned context, not the
    /// original closure fields.
    pub fn execute_bytecode_saving_stack(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        location_map: &Rc<LocationMap>,
    ) -> ExecResult {
        // Save the caller's stack and active allocator (Package 4 plumbing;
        // the allocator pointer is write-only until Package 5 activates it).
        let saved_stack = std::mem::take(&mut self.fiber.stack);
        let saved_allocator = crate::value::fiber_heap::save_active_allocator();

        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();
        let mut current_location_map = location_map.clone();

        let result = loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                0,
                &current_location_map,
            );

            if bits != SIG_OK {
                break ExecResult {
                    bits,
                    ip,
                    bytecode: current_bytecode,
                    constants: current_constants,
                    env: current_env,
                    location_map: current_location_map,
                };
            }

            if let Some(tail) = self.pending_tail_call.take() {
                current_bytecode = tail.bytecode;
                current_constants = tail.constants;
                current_env = tail.env;
                current_location_map = tail.location_map;
            } else {
                break ExecResult {
                    bits,
                    ip,
                    bytecode: current_bytecode,
                    constants: current_constants,
                    env: current_env,
                    location_map: current_location_map,
                };
            }
        };

        // Restore the caller's stack and active allocator
        self.fiber.stack = saved_stack;
        crate::value::fiber_heap::restore_active_allocator(saved_allocator);

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
        location_map: &Rc<LocationMap>,
    ) -> Result<Value, String> {
        self.error_loc = None;

        let mut current_bytecode = bytecode.clone();
        let mut current_constants = constants.clone();
        let mut current_env = closure_env.clone();
        let mut current_location_map = location_map.clone();

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
