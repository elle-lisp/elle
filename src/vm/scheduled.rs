// audited: 2026-10-06
//! VM entry points that connect bytecode execution to the async scheduler.
//!
//! docs/impl/vm.md

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::pipeline::CompileCtx;
use crate::value::{CodeArena, CodeUnit, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT};

use super::core::VM;

impl VM {
    /// Handle a SIG_SWITCH signal: execute the pending fiber resume
    /// and resume the caller with the result. Returns the new signal bits.
    pub(super) fn handle_sig_switch(&mut self) -> SignalBits {
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

        self.finalize_if_halted(&pending.handle, result_bits);
        if result_bits.intersects(SIG_ERROR) {
            pending
                .handle
                .with_mut(|f| f.status = crate::value::FiberStatus::Error);
        }

        if self.absorbs(&pending.handle, mask, result_bits, result_value) {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.resume_suspended(caller_frames, result_value)
        } else {
            self.fiber.signal = Some((result_bits, result_value));

            // Rebuild fiber.suspended for uncaught signals: the outer code
            // (execute_scheduled, execute_code) needs the suspension chain
            // to resume after handling the signal (e.g., SIG_IO → sync I/O).
            // Prepend a FiberResume frame so resume_suspended can re-enter
            // the child fiber when the signal is handled.
            if !result_bits.intersects(SIG_ERROR) && !result_bits.intersects(SIG_HALT) {
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

    /// Run a compiled unit under the async scheduler.
    ///
    /// Wraps the unit's entry function in a thunk and calls `(ev/run thunk)`
    /// to install the async scheduler. The thunk carries the entry's inferred
    /// signal so fiber scheduling and shared allocator provisioning work
    /// correctly.
    ///
    /// Falls back to direct execution if stdlib isn't loaded yet.
    ///
    /// A unit compiled on another heap runs from a copy on this VM's heap, as
    /// `execute` does, and the copy is held for the run.
    pub fn execute_scheduled(
        &mut self,
        unit: &CodeUnit,
        cctx: &CompileCtx,
    ) -> Result<Value, String> {
        let ev_run = match cctx.lookup_stdlib_value(crate::value::SymbolId::of("ev/run")) {
            Some(v) => v,
            None => return self.execute(unit),
        };
        // The entry thunk's code object is the unit's entry: it runs the
        // top-level bytecode, so it carries the real program's location table,
        // child table, and builder-idiom merge metadata
        // (docs/impl/region/merging.md § Merging). Without them the top-level
        // merge would diverge from the unit/embedding paths, which carry it.
        let unit = unit.on_heap(self.heap());

        let call_region = crate::lir::lower::new_static_region();
        // The synthetic `Call` below is hand-encoded bytecode, so the slot is
        // written as its raw wire-format `u32` (the one legit `.get()` site —
        // a bytecode encoder).
        let call_region_slot = call_region.get();
        let synthetic_bc = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::LoadConst as u8,
            0,
            1,
            Instruction::Call as u8,
            0,
            1, // arg_count = 1 (u16be)
            (call_region_slot >> 24) as u8,
            (call_region_slot >> 16) as u8,
            (call_region_slot >> 8) as u8,
            (call_region_slot & 0xff) as u8, // region_id (u32be)
            Instruction::Return as u8,
        ];

        // The entry thunk gets a runtime region of its own, minted from the heap.
        // The static slot baked into the synthetic `Call` above is a compile-time
        // name from a different id-space (docs/impl/region/model.md § id-spaces):
        // read as a physical id it names whichever live region already answers to
        // that number, so the thunk would land among another value's objects and
        // the mint's creation claim would never be taken. A mint takes that claim
        // here, and the release after the run balances it.
        let entry_region = self.heap().new_runtime_region();
        // Build the entry thunk as an ordinary allocation into `entry_region`
        // (mortal) — reclaimed by the termination sweep. The synthetic
        // `(ev/run thunk)` bytecode has no MakeClosure of its own; the real
        // program's nested lambdas sit in the entry payload's child table and
        // resolve when `ev/run` calls the thunk. The thunk's header takes a
        // counted reference to the unit's code region at its allocation, and
        // the wrapper's allocating opcodes resolve their own static region
        // slots.
        let thunk = {
            let heap = self.heap();
            let template = crate::value::build::template(heap, unit.entry(), entry_region);
            crate::value::build::closure(
                heap,
                crate::value::Closure::new(
                    crate::value::TemplateRef::region(template),
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                ),
                entry_region,
            )
        };
        // The synthetic `(ev/run thunk)` wrapper has no allocations and no
        // releases of its own; the real program's tables ride the thunk's code
        // object and resolve when `ev/run` calls it.
        let mut wrapper = Bytecode::new();
        wrapper.instructions = synthetic_bc;
        wrapper.constants = vec![thunk, ev_run];
        let wrapper = CodeUnit::new(CodeArena::mint(self.heap()), wrapper);
        let result = self.execute(&wrapper);
        // The run is over, so this is the entry thunk's point of demise (Rule 4,
        // docs/impl/region/rules.md): the wrapper's hand-encoded bytecode carries
        // no `DecrefRegion` to fire, so the balance for the mint above is here.
        // The result of `(ev/run thunk)` lives in the fresh region that call
        // minted, never in this one, so the release cannot reach the value being
        // returned. It runs on the error exit too — a failed run owes the same
        // balance as a completed one.
        self.heap().decref_region(entry_region);
        result
    }
}
