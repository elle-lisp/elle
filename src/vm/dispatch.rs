//! Main instruction dispatch loop.
//!
//! This module contains the core bytecode execution loop that dispatches
//! instructions to their handlers.

use crate::compiler::bytecode::Instruction;
use crate::error::LocationMap;
use crate::value::{
    BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_FUEL, SIG_HALT, SIG_OK,
};
use std::rc::Rc;

use super::core::VM;
use super::{
    arithmetic, capture, closure, comparison, control, data, literals, stack, types, variables,
};

mod interp;
mod region;

impl VM {
    /// Handle the Emit instruction.
    ///
    /// For error signals (SIG_ERROR): stores the error in fiber.signal and
    /// exits the dispatch loop. No SuspendedFrame — errors propagate through
    /// the normal return/unwind path.
    ///
    /// For suspension signals (SIG_YIELD, user-defined): captures a
    /// SuspendedFrame so that resume can continue from this exact point.
    fn handle_emit(
        &mut self,
        signal_bits: SignalBits,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: usize,
    ) -> (SignalBits, usize) {
        let value = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on emit");

        // The compiler emits a `DecrefRegion` at the Emit's `decref_point`
        // HirId, which fires right after this handler
        // returns. Incref the value's region here so the matching
        // decref doesn't take RC to zero while the scheduler still
        // holds the value via `fiber.signal`.
        let heap = unsafe { &mut *self.heap_ptr };
        let value_region = crate::value::arena::region_of(heap, value);
        crate::value::arena::incref_for_escape(
            heap,
            value_region,
            crate::value::arena::EscapeSite::EmitEscape,
        );

        self.fiber.signal = Some((signal_bits, value));

        if !signal_bits.intersects(SIG_ERROR) {
            // Suspension: save stack and create a frame for later resumption.
            let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();

            // Innermost yield frame: this activation's remap is still on top
            // (the wrapping `saving_stack` pops it after we return).
            let activation_region_map = self
                .fiber
                .activation_region_maps
                .last()
                .cloned()
                .unwrap_or_default();
            // MOVE the activation's owner node into the frame (its slot is
            // likewise still on top): the members are Owned with no other
            // release route, so the node must ride the park to the resumed
            // body's completion (docs/impl/region/owner.md § "Owner nodes").
            let activation_owner_node = self.take_activation_owner_node();
            // The yielding activation runs the current closure — park it so the
            // recursion resolves to the same closure after resume.
            let current_closure = self.fiber.current_closure;
            let frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                code.clone(),
                closure_env.clone(),
                ip,
                saved_stack,
                true,
                activation_region_map,
                activation_owner_node,
                current_closure,
                self.heap(),
            ));
            self.fiber.suspended = Some(vec![frame]);
        }

        (signal_bits, ip)
    }
}
