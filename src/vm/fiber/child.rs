use super::*;

impl VM {
    /// Execute a closure with the child fiber swapped in as the active fiber.
    ///
    /// Handles the full swap protocol: take child from handle, wire
    /// parent/child chain, swap fibers, run the closure, update status,
    /// extract result, swap back, put child back.
    ///
    /// `child_value` is the cached Value wrapping the child's FiberHandle,
    /// cached on the parent so `fiber/child` can return it without re-allocating.
    ///
    /// Returns `(signal_bits, signal_value)` from the child's execution.
    pub(super) fn with_child_fiber(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
        execute: impl FnOnce(&mut VM) -> SignalBits,
    ) -> (SignalBits, Value) {
        // 1. Take child fiber out of its handle (sets slot to None)
        let mut child_fiber = child_handle.take();

        // 2. Wire up parent/child chain (Janet semantics). A trampolined
        //    nested resume descends from the ROOT context — the fiber whose
        //    code called fiber/resume is swapped out by then — so the true
        //    parent arrives via `trampoline_parent_override`; otherwise the
        //    currently active fiber is the parent.
        self.fiber.child = Some(child_handle.clone());
        self.fiber.child_value = Some(child_value);
        if let Some((parent_handle, parent_value)) = self.trampoline_parent_override.take() {
            child_fiber.parent = Some(parent_handle.downgrade());
            child_fiber.parent_value = parent_value;
        } else {
            child_fiber.parent = self.current_fiber_handle.as_ref().map(|h| h.downgrade());
            child_fiber.parent_value = self.current_fiber_value;
        }

        // 2a. Propagate withheld capabilities: child inherits parent's withheld.
        // This is idempotent (OR is monotonic) so safe on repeated resume.
        child_fiber.withheld |= self.fiber.withheld;

        // 3. Swap parent out, child in; track the child's handle and value
        let parent_handle = self.current_fiber_handle.take();
        let parent_value = self.current_fiber_value.take();
        self.current_fiber_handle = Some(child_handle.clone());
        self.current_fiber_value = Some(child_value);
        std::mem::swap(&mut self.fiber, &mut child_fiber);

        // 3a. With the unified heap, no swap is needed — all fibers share
        //     the VM's single heap.

        // 4. Execute the closure
        let bits = execute(self);

        // 5. Update child status based on result.
        //    SIG_OK is terminal (Dead). Other signals are Suspended; the
        //    caller decides whether a caught SIG_ERROR stays Suspended
        //    (resumable) or gets promoted to Error (terminal) based on
        //    the parent's mask. SIG_HALT is also provisionally Suspended
        //    here — the resume handler promotes to Dead if the mask
        //    doesn't catch it (`finalize_dead_fiber`).
        self.fiber.status = if bits.is_ok() {
            FiberStatus::Dead
        } else {
            FiberStatus::Paused
        };

        // 5a. Dead is terminal: free everything the completed fiber owns —
        //     the fiber owner node (and, defensively, any leftover parked
        //     chain's activation nodes; a completing resume consumes the
        //     chain) — before the terminal result is pinned below
        //     (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown
        //     frees everything the fiber owns"). The child is the live
        //     `self.fiber` here, so the take needs no handle borrow.
        if bits.is_ok() {
            let owned = super::take_fiber_owned(&mut self.fiber);
            super::release_fiber_owned(unsafe { &mut *self.heap_ptr }, owned);
        }

        // 6. Extract the result before swapping back.
        //    Deep-copy any private-pool values to the outbox so the parent
        //    doesn't read dangling pointers.  Two cases:
        //      a) result_value itself is in the private pool — deep-copy it.
        //      b) result_value is in the outbox but contains nested references
        //         to the private pool (e.g. yield [:send target msg] where
        //         msg was allocated before OutboxEnter) — deep-copy it so
        //         nested values are relocated too.
        let result_value = self
            .fiber
            .signal
            .as_ref()
            .map(|(_, v)| *v)
            .unwrap_or(Value::NIL);
        let result_bits = self.fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(bits);

        // 6a. Park-retain for TERMINAL results (return / error / halt). Such a
        //     fiber holds its result in `signal`, read later via `fiber/value`
        //     after control has left it — so the parent's `DecrefValueRegion`
        //     on the resume result must not free it out from under the fiber.
        //     Pin its region; the matching release is the `signal` scan in
        //     `find_object_cross_refs`'s Fiber arm when the fiber is freed. Yield /
        //     other suspending signals are excluded: their value is consumed
        //     transiently by the resumer and the fiber will run again, so the
        //     normal value flow already governs it (retaining here would leak).
        if is_terminal_signal(result_bits) {
            incref_signal_region(unsafe { &mut *self.heap_ptr }, &self.fiber.signal);
            // Record the matching outgoing content edge `fiber-region → result-region`
            // (docs/impl/region/ownership.md § "The outgoing edge table"): the scan's Fiber
            // arm reads this terminal `signal` value, so it is a content edge the
            // free-time walk must release when the fiber frees. There is no explicit
            // un-record — a terminal fiber is read (`fiber/value`), never resumed, the
            // same asymmetric park-retain the RC incref above takes.
            let heap = unsafe { &mut *self.heap_ptr };
            let fiber_r = crate::value::arena::region_of(heap, child_value);
            let sig_r = self
                .fiber
                .signal
                .as_ref()
                .and_then(|&(_, v)| crate::value::arena::region_of(heap, v));
            heap.record_outgoing_edge(fiber_r, sig_r);
        }

        // 7. Swap back: parent in, child out; restore handle
        std::mem::swap(&mut self.fiber, &mut child_fiber);
        self.current_fiber_handle = parent_handle;
        self.current_fiber_value = parent_value;

        // 8. Put child fiber back into its handle
        child_handle.put(child_fiber);

        (result_bits, result_value)
    }
}
