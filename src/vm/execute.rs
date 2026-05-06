//! Bytecode execution entry points.
//!
//! ## Re-entrancy
//!
//! `execute_bytecode_saving_stack` makes the VM re-entrant. It is called
//! recursively from within the dispatch loop in several places:
//!
//! | Caller | Context |
//! |--------|---------|
//! | `eval` primitive | Compiles and runs Elle source from within running code |
//! | Non-yielding `fiber/resume` | Runs a child fiber inline on the current thread |
//! | `arena/allocs` SIG_QUERY handler | Runs a thunk to measure its allocations |
//! | JIT trampolines | Re-enters interpreter for uncompiled hot paths |
//! | Coroutine resume in `call.rs` | Resumes a suspended coroutine |
//!
//! ### What `execute_bytecode_saving_stack` preserves
//!
//! - **Operand stack**: saved before inner execution, restored after. The
//!   inner execution sees an empty stack. The outer stack is invisible to it.
//! - **Active allocator pointer**: saved and restored. Inner execution uses
//!   whatever allocator was active (scope bumps, shared allocator, etc.).
//!
//! ### What it does NOT preserve
//!
//! - **`self.fiber.signal`**: the inner execution overwrites this with its
//!   result. Callers must read `fiber.signal` immediately after return and
//!   before any other operation that might set it.
//! - **`self.fiber.frames` / `self.fiber.call_stack`**: inner calls push
//!   and pop frames. On normal return these are balanced. On error they
//!   may be partially unwound.
//! - **`self.error_loc`**: overwritten by inner execution on error.
//! - **`self.pending_tail_call`**: consumed by the tail-call loop inside
//!   `execute_bytecode_saving_stack`. Never leaks to the outer caller.
//!
//! ### Yield from inner execution
//!
//! If the inner closure yields (`SIG_YIELD`), `execute_bytecode_saving_stack`
//! returns `SIG_YIELD` to its caller. The saved outer stack is restored, but
//! the fiber is now suspended mid-inner-execution. **This is a bug in any
//! caller that does not handle `SIG_YIELD`.** Current callers that call
//! user-provided closures (`eval`, `arena/allocs`) do not handle yield —
//! they propagate the signal upward, which will confuse the outer execution
//! context. Closures passed to these primitives must be non-yielding (Pure
//! signal). This is not currently enforced at the call site.
//!
//! ### Rules for new callers
//!
//! If you add a new SIG_QUERY handler or primitive that calls a user closure
//! via `execute_bytecode_saving_stack`:
//!
//! 1. Read `fiber.signal` immediately after return to get the result.
//! 2. Check `exec_result.bits` for `SIG_ERROR` and `SIG_HALT` before using
//!    the result.
//! 3. Do NOT call it with a closure that may yield unless you handle
//!    `SIG_YIELD` in the return value.
//! 4. Do NOT assume `fiber.signal` is unchanged after the call.
//! 5. The inner execution runs on the SAME fiber — same heap, same
//!    parameter frames. It is not isolated.

use crate::error::LocationMap;
use crate::value::{SignalBits, Value, SIG_ERROR};
use std::rc::Rc;

use super::core::VM;

/// Advance pool rotation state at a tail-call boundary.
///
/// If the previous iteration was rotation-safe, either rotate pools
/// (if a base mark exists) or capture a new base mark. If the previous
/// iteration was NOT rotation-safe, clear the base mark to prevent
/// rotating across an unsafe boundary.
#[inline]
pub(super) fn advance_rotation(
    rotation_base: &mut Option<crate::value::fiberheap::RotationBase>,
    prev_rotation_safe: &mut bool,
    tail_rotation_safe: bool,
) {
    if *prev_rotation_safe {
        if let Some(ref base) = rotation_base {
            crate::value::fiberheap::with_current_heap_mut(|h| h.rotate_pools(base));
        } else {
            *rotation_base = crate::value::fiberheap::with_current_heap_mut(|h| h.rotation_mark());
        }
    } else {
        *rotation_base = None;
    }
    *prev_rotation_safe = tail_rotation_safe;
}

/// Result of `execute_bytecode_saving_stack`.
///
/// Contains the signal, IP, the active bytecode/constants/env at exit, and
/// the inner operand stack at the moment of suspension.
///
/// When a tail call occurs before a signal, the active context differs from
/// the original closure — callers that create `SuspendedFrame`s must use
/// these fields, not the original closure's bytecode/constants.
///
/// `stack` captures the inner execution's operand stack at suspension time.
/// This is essential for fuel-pause resumption: when `SIG_FUEL` fires at a
/// `TailCall` or `Call` instruction, the args are still on the stack. On
/// resume the instruction re-executes from `ip`, so the stack must be
/// restored exactly as it was.  `SIG_YIELD` is exempt — `handle_yield`
/// drains the stack into `fiber.suspended` before returning, so
/// `fiber.suspended` is already populated and the `stack` field here is
/// unused for that signal.
pub(crate) struct ExecResult {
    pub bits: SignalBits,
    pub ip: usize,
    pub bytecode: Rc<Vec<u8>>,
    pub constants: Rc<Vec<Value>>,
    pub env: Rc<Vec<Value>>,
    pub location_map: Rc<LocationMap>,
    /// The inner operand stack at suspension. Populated by
    /// `execute_bytecode_saving_stack`; empty for `execute_bytecode_from_ip`.
    pub stack: Vec<Value>,
}

impl VM {
    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    ///
    /// Returns `ExecResult` containing the signal, IP, and the active
    /// bytecode/constants/env at exit. The active context may differ from
    /// the input if a tail call occurred before the signal.
    /// Core tail-call trampoline loop shared by `execute_bytecode_from_ip`
    /// and `execute_bytecode_saving_stack`.
    fn trampoline_loop(
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
        let mut accumulated_squelch_mask = SignalBits::EMPTY;
        let mut rotation_base: Option<crate::value::fiberheap::RotationBase> = None;
        let mut prev_rotation_safe = true;

        loop {
            let (bits, ip) = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                &current_env,
                current_ip,
                &current_location_map,
            );

            if !bits.is_ok() {
                if self.enforce_squelch(bits, accumulated_squelch_mask) {
                    break ExecResult {
                        bits: SIG_ERROR,
                        ip,
                        bytecode: current_bytecode,
                        constants: current_constants,
                        env: current_env,
                        location_map: current_location_map,
                        stack: vec![],
                    };
                }
                let inner_stack = std::mem::take(&mut self.fiber.stack).into_vec();
                break ExecResult {
                    bits,
                    ip,
                    bytecode: current_bytecode,
                    constants: current_constants,
                    env: current_env,
                    location_map: current_location_map,
                    stack: inner_stack,
                };
            }

            if let Some(tail) = self.pending_tail_call.take() {
                advance_rotation(
                    &mut rotation_base,
                    &mut prev_rotation_safe,
                    tail.rotation_safe,
                );
                accumulated_squelch_mask |= tail.squelch_mask;
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
                    stack: vec![],
                };
            }
        }
    }

    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    pub(crate) fn execute_bytecode_from_ip(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> ExecResult {
        self.trampoline_loop(bytecode, constants, closure_env, start_ip, location_map)
    }

    /// Execute bytecode returning SignalBits (for fiber/closure execution).
    /// The result value is stored in `self.fiber.signal`.
    ///
    /// Saves/restores the caller's stack around execution.
    /// Handles pending tail calls in a loop.
    pub(crate) fn execute_bytecode_saving_stack(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        location_map: &Rc<LocationMap>,
    ) -> ExecResult {
        let saved_stack = std::mem::take(&mut self.fiber.stack);
        let result = self.trampoline_loop(bytecode, constants, closure_env, 0, location_map);
        self.fiber.stack = saved_stack;
        result
    }
}
