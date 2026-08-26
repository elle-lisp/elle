//! `fiber/propagate`: re-raise a child fiber's caught signal to this fiber's
//! own parent. Call- and tail-position handlers (see the `super` module doc).

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};
use crate::vm::core::VM;

impl VM {
    /// Read the child's parked signal, minting the DELIVERY reference the
    /// re-park owes. Shared by the call-, tail-, and JIT-position handlers —
    /// all three run the same install, so all three owe the same reference.
    ///
    /// Installing the child's payload as this fiber's own `signal` is a fresh
    /// park: this fiber's resumer reads the payload as its resume result and
    /// runs the compiler-emitted release on it. The child's park funded its own
    /// resumer's release, not this one, so without a mint here that release
    /// consumes a reference nothing took — the payload's count then runs one
    /// short of the recorded `fiber → payload` edges and the free cascade
    /// reclaims it under the caller (docs/impl/region/owner.md § "Park/unpark
    /// symmetry").
    ///
    /// Three cases take no mint, each because the delivery already has an owner
    /// or no consumer:
    ///
    /// - A NON-TERMINAL signal (a yield, an io request). The fiber runs again,
    ///   so the resume path proper governs the payload — `release_parked_signal`
    ///   for an io request, the resumed body's own pending release otherwise.
    ///   `with_child_fiber` step 6a excludes exactly this set from its park
    ///   retain for the same reason ("retaining here would leak"), and the
    ///   delivery follows the park.
    /// - `SIG_HALT`, for the reason `VM::handle_emit` skips it: a halt promotes
    ///   the fiber to `:dead`, `fiber/resume` refuses it, so that delivery has
    ///   no consumer and a retain would strand the payload.
    /// - The no-signal fallback, whose error `escaping_error` builds in a fresh
    ///   region already carrying the one reference the consumer's
    ///   `DecrefValueRegion` releases — only a BORROWED payload is unfunded.
    pub(super) fn take_propagated_signal(
        &mut self,
        handle: &crate::value::FiberHandle,
    ) -> (SignalBits, Value) {
        let Some((bits, value)) = handle.with(|fiber| fiber.signal) else {
            return (
                SIG_ERROR,
                self.escaping_error("internal-error", "fiber/propagate: no signal"),
            );
        };
        // Re-raising the child's error resumes the propagation the catch ended,
        // so the location parked with the payload becomes live again. This is
        // what lets `defer` and the scheduler report the form that raised,
        // rather than the form that re-raised (docs/impl/vm.md § "Where a
        // reported error's location comes from"). The parked pair is taken only
        // when it still names THIS payload; a record the fiber kept from an
        // earlier error describes nothing here.
        if bits.intersects(SIG_ERROR) && self.error_loc.is_none() {
            self.error_loc = handle.with(|fiber| {
                fiber
                    .error_loc
                    .as_ref()
                    .filter(|(payload, _)| payload.bit_identical(value))
                    .map(|(_, loc)| loc.clone())
            });
        }
        if super::is_terminal_signal(bits) && !bits.intersects(SIG_HALT) {
            let heap = unsafe { &mut *self.heap_ptr };
            let region = crate::value::arena::region_of(heap, value);
            crate::value::arena::incref_for_escape(
                heap,
                region,
                crate::value::arena::EscapeSite::PropagateEscape,
            );
        }
        (bits, value)
    }

    /// Handle SIG_PROPAGATE from fiber/propagate (Call position).
    pub(in crate::vm) fn handle_fiber_propagate_signal(
        &mut self,
        fiber_value: Value,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_PROPAGATE with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (child_bits, child_value) = self.take_propagated_signal(&handle);

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.intersects(SIG_ERROR) || child_bits.intersects(SIG_HALT) {
            self.fiber.stack.push(Value::NIL);
            None
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            self.set_error(
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            self.fiber.stack.push(Value::NIL);
            None
        } else {
            Some(child_bits)
        }
    }

    /// Handle SIG_PROPAGATE from fiber/propagate (TailCall position).
    pub(in crate::vm) fn handle_fiber_propagate_signal_tail(
        &mut self,
        fiber_value: Value,
    ) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_PROPAGATE with non-fiber value");
                return SIG_ERROR;
            }
        };

        let (child_bits, child_value) = self.take_propagated_signal(&handle);

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.intersects(SIG_ERROR) || child_bits.intersects(SIG_HALT) {
            child_bits
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            self.set_error(
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            SIG_ERROR
        } else {
            child_bits
        }
    }
}
