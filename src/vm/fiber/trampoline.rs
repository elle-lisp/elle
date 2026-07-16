//! The SIG_SWITCH trampoline driving nested `fiber/resume` iteratively rather
//! than on the Rust stack. `finish_fiber_resume` is the shared completion driver
//! for `do_fiber_resume` and `do_fiber_abort` (see their docs in `super`).

use crate::value::fiber::FiberStatus;
use crate::value::{
    FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_SWITCH,
    SIG_TERMINAL,
};

use crate::vm::core::VM;

impl VM {
    /// Drive the SIG_SWITCH trampoline to a real signal.
    ///
    /// Entered with the result of executing `child` (`bits`/`value`); if the
    /// child suspended to resume a deeper fiber (SIG_SWITCH +
    /// `pending_fiber_resume`), descend iteratively and unwind results back
    /// up — never recursing on the Rust stack. Also the completion driver
    /// for OTHER fiber executors whose frame replay can surface SIG_SWITCH
    /// (`do_fiber_abort`): they pass their `with_child_fiber` result here.
    pub(super) fn finish_fiber_resume(
        &mut self,
        mut bits: SignalBits,
        mut value: Value,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        // Fast path: no nested fiber/resume — return directly.
        if bits != SIG_SWITCH {
            return (bits, value);
        }

        // Slow path: trampoline for nested fiber/resume.
        //
        // fiber_stack records fibers that suspended because they called
        // fiber/resume on a deeper fiber. We iterate instead of recursing.
        let mut fiber_stack: Vec<(FiberHandle, Value)> = vec![];
        fiber_stack.push((child_handle.clone(), child_value));

        loop {
            if bits == SIG_SWITCH {
                // A fiber called fiber/resume on a child: descend.
                let pending = self
                    .pending_fiber_resume
                    .take()
                    .expect("VM bug: SIG_SWITCH without pending_fiber_resume");

                // Install the true parent for with_child_fiber's wiring —
                // we descend from the root context, but the child's parent
                // is the fiber that requested this resume.
                self.trampoline_parent_override = pending.parent;

                fiber_stack.push((pending.handle.clone(), pending.fiber_value));
                let (new_bits, new_value) =
                    self.do_fiber_resume_single(&pending.handle, pending.fiber_value);
                // Clear any override an early-return path (e.g. native
                // iterators) left unconsumed, so it cannot mis-wire a later
                // unrelated with_child_fiber.
                self.trampoline_parent_override = None;
                bits = new_bits;
                value = new_value;
                continue;
            }

            // Real signal from the current deepest fiber. Unwind the stack.
            loop {
                let (current_handle, current_fv) = fiber_stack.pop().unwrap();
                let mask = current_handle.with(|f| f.mask);

                if bits.contains(SIG_HALT) {
                    self.finalize_dead_fiber(&current_handle);
                }

                let caught = bits.is_ok() || (mask.covers(bits) && !bits.contains(SIG_TERMINAL));

                if caught {
                    self.fiber.child = None;
                    self.fiber.child_value = None;

                    // Update the fiber's signal so fiber/value returns the
                    // correct result to Elle code. For the fiber whose own
                    // execution produced (bits, value) this re-installs the
                    // pair `with_child_fiber` already parked — retained and
                    // edge-recorded at its step 6a — so nothing more is owed.
                    // But an ANCESTOR catching a propagated TERMINAL signal
                    // (a capability denial masked by the grandparent, not the
                    // parent) receives a terminal payload it never held: park
                    // it with the same retain + recorded content edge 6a
                    // takes, or the fiber's free-time signal scan finds an
                    // edge the recorded table lacks (the equivalence oracle's
                    // drift) and its cascade decref over-releases the payload.
                    let needs_park = current_handle.with(|f| f.signal != Some((bits, value)))
                        && super::is_terminal_signal(bits);
                    current_handle.with_mut(|f| {
                        f.signal = Some((bits, value));
                    });
                    if needs_park {
                        let heap = unsafe { &mut *self.heap_ptr };
                        super::refcount::incref_signal_region(heap, &Some((bits, value)));
                        let fiber_r = crate::value::arena::region_of(heap, current_fv);
                        let sig_r = crate::value::arena::region_of(heap, value);
                        heap.record_outgoing_edge(fiber_r, sig_r);
                    }

                    if fiber_stack.is_empty() {
                        // Back to the original caller.
                        return (bits, value);
                    }

                    // Drop our strong handle on the completed child BEFORE
                    // re-entering the parent: the parent's continuation may
                    // run to completion inside this call (for the root fiber
                    // it is the rest of the program). A live strong handle
                    // here keeps the dead child's fiber state upgradeable,
                    // and a `fiber/parent` rebuild during that window scans
                    // (increfs) through state whose mirror decref at free
                    // time is skipped once the handle finally dies — leaking
                    // the child's region chain.
                    drop(current_handle);

                    // Resume the parent fiber: it was suspended waiting
                    // for this child to complete. Clear its child wiring
                    // (set by seed_child_inheritance at suspension) just as
                    // the recursion-era caught path did, and deliver the
                    // child's result as the resume value.
                    let (parent_handle, parent_fv) = fiber_stack.last().unwrap();
                    parent_handle.with_mut(|f| {
                        f.child = None;
                        f.child_value = None;
                        f.signal = Some((SIG_OK, value));
                    });

                    let (new_bits, new_value) =
                        self.do_fiber_resume_single(parent_handle, *parent_fv);
                    bits = new_bits;
                    value = new_value;
                    // Break to outer loop to check for SIG_SWITCH.
                    break;
                } else {
                    // Signal NOT caught — propagate through fiber stack.
                    if bits.contains(SIG_ERROR) {
                        current_handle.with_mut(|f| f.status = FiberStatus::Error);
                    }

                    if fiber_stack.is_empty() {
                        return (bits, value);
                    }

                    // For uncaught suspending signals (e.g. SIG_IO), build
                    // FiberResume frame on the parent so the suspension chain
                    // preserves the fiber nesting for re-entry.
                    if !bits.contains(SIG_ERROR) && !bits.contains(SIG_HALT) {
                        let (parent_handle, _) = fiber_stack.last().unwrap();
                        let child_resume_frame = SuspendedFrame::FiberResume {
                            handle: current_handle.clone(),
                            fiber_value: current_fv,
                        };
                        parent_handle.with_mut(|f| {
                            let mut new_frames = vec![child_resume_frame];
                            if let Some(mut existing) = f.suspended.take() {
                                new_frames.append(&mut existing);
                            }
                            f.suspended = Some(new_frames);
                            f.signal = Some((bits, value));
                        });
                    }
                    // Continue unwinding to the next parent.
                }
            }
        }
    }
}
