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

        // The DELIVERY reference: the resumer reads this value out of
        // `fiber.signal` as its resume result and releases it there, so the park
        // mints the reference that release consumes — the same service a
        // completing child's `Return` mint performs for a terminal result. The
        // body's own reference is a separate one, released by the continuation
        // past this suspend (docs/impl/region/owner.md § "Park/unpark symmetry" —
        // "A fiber body owns one reference of every value it yields").
        //
        // A HALT is the one signal whose decref never fires, so it is the one
        // signal that must not be retained. The dispatch loop leaves at this
        // emit, and every position that drives a child routes a halt through
        // `VM::finalize_if_halted`, which promotes the fiber to `:dead` — which
        // `fiber/resume` refuses. So the instruction after this one is
        // unreachable and there is no consumer for the retain: taking it strands
        // the payload's region, one per halted fiber. What pins the payload
        // meanwhile is the park retain the resume takes for a terminal result
        // (`incref_signal_region`, child.rs step 6a), exactly as it does for the
        // bare `Return` a `SIG_OK` body leaves through — which likewise reaches
        // `fiber.signal` unretained.
        if !signal_bits.intersects(SIG_HALT) {
            let heap = unsafe { &mut *self.heap_ptr };
            let value_region = crate::value::arena::region_of(heap, value);
            crate::value::arena::incref_for_escape(
                heap,
                value_region,
                crate::value::arena::EscapeSite::EmitEscape,
            );
            // An ERROR emit's retain is the payload's whole delivery, so the
            // raise chain's own reference funds nothing: record the mint so the
            // abandoned-frame walk and the parked frame's discharge stop
            // exempting the payload's region and reclaim that reference
            // (docs/impl/region/mechanism.md § "An abandoned frame runs the
            // releases it still owes").
            if signal_bits.intersects(SIG_ERROR) {
                self.fiber.delivery.record_mint(value);
            }
        }

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
            // MOVE what the activation owes into the frame (its slot is
            // likewise still on top): a node's members are Owned with no other
            // release route and a deferred tail-call release has no emitted
            // instruction left at all, so both must ride the park to the resumed
            // body's completion (docs/impl/region/owner.md § "Owner nodes").
            let activation_dues = self.take_activation_dues();
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
                activation_dues,
                current_closure,
                self.heap(),
            ));
            self.fiber.suspended = Some(vec![frame]);
        }

        (signal_bits, ip)
    }
}
