pub mod arithmetic;
pub mod call;
pub mod capture;
// Note: jit_entry is not pub — it only adds impl VM methods
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod dispatch;
pub mod env;
pub mod eval;
pub mod execute;
pub mod fiber;
mod jit_entry;
pub mod literals;
#[cfg(feature = "mlir")]
mod mlir_entry;
pub mod parameters;
pub mod signal;
pub mod stack;
pub mod types;
pub mod variables;
#[cfg(feature = "wasm")]
mod wasm_entry;

pub use crate::value::fiber::CallFrame;
pub use core::VM;

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::pipeline::lookup_stdlib_value;
use crate::symbol::SymbolTable;
use crate::value::{
    error_val, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_SWITCH, SIG_YIELD,
};
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

        // Initial execution with tail-call loop.
        // Pool rotation: when a tail call is rotation-safe, release the
        // previous iteration's temporaries via rotate_pools(). The tail
        // call's env (arguments) was built before release, so referenced
        // values survive. Only unreferenced temporaries are freed.
        let mut bits;
        let mut rotation_base: Option<crate::value::fiberheap::RotationBase> = None;
        let mut prev_rotation_safe = true;
        loop {
            let (b, _ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                0,
                &current_location_map,
            );
            bits = b;
            if let Some(tail) = self.pending_tail_call.take() {
                if prev_rotation_safe {
                    if let Some(ref base) = rotation_base {
                        crate::value::fiberheap::with_current_heap_mut(|h| h.rotate_pools(base));
                    } else {
                        rotation_base =
                            crate::value::fiberheap::with_current_heap_mut(|h| h.rotation_mark());
                    }
                } else {
                    rotation_base = None;
                }
                prev_rotation_safe = tail.rotation_safe;

                current_bytecode = tail.bytecode;
                current_constants = tail.constants;
                current_env = tail.env;
                current_location_map = tail.location_map;
            } else {
                break;
            }
        }

        // Signal handling loop — handles SIG_SWITCH iteratively.
        loop {
            if bits.is_ok() || bits == SIG_HALT {
                let (_, value) = self.fiber.signal.take().unwrap();
                return Ok(value);
            } else if bits.contains(SIG_ERROR) {
                let (_, err_value) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                return Err(self.format_error_with_location(err_value));
            } else if bits == SIG_SWITCH {
                bits = self.handle_sig_switch();
            } else if bits.contains(SIG_YIELD) {
                return Err("Unexpected yield outside coroutine context".to_string());
            } else {
                self.fiber.signal.take();
                return Err(format!(
                    "Unexpected signal outside coroutine context: {}",
                    bits
                ));
            }
        }
    }

    /// Handle a SIG_SWITCH signal: execute the pending fiber resume
    /// and resume the caller with the result. Returns the new signal bits.
    fn handle_sig_switch(&mut self) -> SignalBits {
        let pending = self
            .pending_fiber_resume
            .take()
            .expect("VM bug: SIG_SWITCH without pending_fiber_resume");
        let caller_frames = self.fiber.suspended.take().unwrap_or_default();
        self.fiber.signal.take();
        if self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::FIBER)
        {
            eprintln!(
                "[handle_sig_switch] caller_frames={} fiber_status={:?}",
                caller_frames.len(),
                pending.handle.with(|f| f.status),
            );
        }

        let (result_bits, result_value) =
            self.do_fiber_resume(&pending.handle, pending.fiber_value);

        let mask = pending.handle.with(|f| f.mask);

        if result_bits.contains(SIG_HALT) {
            pending
                .handle
                .with_mut(|f| f.status = crate::value::FiberStatus::Dead);
        }
        if result_bits.contains(SIG_ERROR) {
            pending
                .handle
                .with_mut(|f| f.status = crate::value::FiberStatus::Error);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(crate::value::SIG_TERMINAL));

        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.resume_suspended(caller_frames, result_value)
        } else {
            self.fiber.signal = Some((result_bits, result_value));

            // Rebuild fiber.suspended for uncaught signals: the outer code
            // (execute_scheduled, execute_bytecode) needs the suspension chain
            // to resume after handling the signal (e.g., SIG_IO → sync I/O).
            // Prepend a FiberResume frame so resume_suspended can re-enter
            // the child fiber when the signal is handled.
            if !result_bits.contains(SIG_ERROR) && !result_bits.contains(SIG_HALT) {
                let fiber_resume_frame = SuspendedFrame::FiberResume {
                    handle: pending.handle.clone(),
                    fiber_value: pending.fiber_value,
                };
                let mut frames = vec![fiber_resume_frame];
                frames.extend(caller_frames);
                self.fiber.suspended = Some(frames);
            }

            result_bits
        }
    }

    /// Execute user bytecode under the async scheduler.
    ///
    /// Wraps the bytecode in a thunk and calls `(ev/run thunk)` to
    /// install the async scheduler. The thunk carries the bytecode's
    /// inferred signal so fiber scheduling and shared allocator
    /// provisioning work correctly.
    ///
    /// Falls back to direct execution if stdlib isn't loaded yet.
    pub fn execute_scheduled(
        &mut self,
        bytecode: &Bytecode,
        symbols: &SymbolTable,
    ) -> Result<Value, String> {
        let ev_run_id = match symbols.get("ev/run") {
            Some(id) => id,
            None => return self.execute(bytecode),
        };
        let ev_run = match lookup_stdlib_value(ev_run_id) {
            Some(v) => v,
            None => return self.execute(bytecode),
        };

        let thunk = Value::closure(crate::value::Closure {
            template: Rc::new(crate::value::ClosureTemplate {
                bytecode: Rc::new(bytecode.instructions.to_vec()),
                arity: crate::value::Arity::Exact(0),
                num_locals: 0,
                num_captures: 0,
                num_params: 0,
                constants: Rc::new(bytecode.constants.to_vec()),
                signal: bytecode.signal,
                capture_params_mask: 0,
                capture_locals_mask: 0,

                symbol_names: Rc::new(std::collections::HashMap::new()),
                location_map: Rc::new(bytecode.location_map.clone()),
                rotation_safe: false,
                lir_function: None,
                doc: None,
                syntax: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
                result_is_immediate: false,
                has_outward_heap_set: false,
                wasm_func_idx: None,
                spirv: std::cell::OnceCell::new(),
            }),
            env: Rc::new(vec![]),
            squelch_mask: SignalBits::EMPTY,
        });

        let synthetic_bc = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::LoadConst as u8,
            0,
            1,
            Instruction::Call as u8,
            0,
            1, // arg_count as u16be
            Instruction::Return as u8,
        ];
        let synthetic_constants = vec![thunk, ev_run];

        self.location_map = bytecode.location_map.clone();
        self.execute_bytecode(&synthetic_bc, &synthetic_constants, None)
    }
}
