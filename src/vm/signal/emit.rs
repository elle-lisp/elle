// audited: 2026-10-06
//! The capability gate on a literal `emit`: an `Emit` of a withheld bit raises nothing, and parks on a denial.
//!
//! docs/signals/emit.md
//! docs/impl/region/park.md
//!
//! The literal form compiles to the `Emit` instruction rather than a call, so no
//! call-site gate sees it, and each tier's `Emit` handler asks here instead: the
//! interpreter's `handle_emit`, the JIT's `elle_jit_yield`, and the WASM host's
//! `rt_emit`. The denial names `fiber/emit`, the primitive the dynamic form calls,
//! so the parent reads one payload from either form.

use crate::value::{SignalBits, Value};

use super::super::core::VM;

impl VM {
    /// The bits an `Emit` of `bits` is denied on a fiber that withholds
    /// `withheld`, the question a call to `fiber/emit` asks of its first
    /// argument (`capability_blocked_for`).
    pub(crate) fn emit_blocked(withheld: SignalBits, bits: SignalBits) -> SignalBits {
        bits.intersection(withheld)
            .intersection(crate::signals::CAP_MASK)
    }

    /// Build the denial payload for an `Emit` of `bits` carrying `value`, of
    /// which `blocked` are withheld. `:args` holds what a call to `fiber/emit`
    /// would have received: the signal as a keyword set, and the value.
    pub(crate) fn build_emit_denial_payload(
        ctx: &mut crate::primitives::ctx::Alloc,
        bits: SignalBits,
        blocked: SignalBits,
        value: Value,
        mut memo: Option<&mut crate::symbol::SymbolTable>,
    ) -> Value {
        let keywords = crate::signals::registry::global_registry()
            .lock()
            .unwrap()
            .bits_to_keywords(bits, memo.as_deref_mut());
        let signal = ctx.set(keywords.into_iter().collect());
        Self::build_denial_payload(
            ctx,
            crate::primitives::fibers::emit_def(),
            blocked,
            &[signal, value],
            memo,
        )
    }

    /// Install the denial an `Emit` of `bits` carrying `value` parks on, of which
    /// `blocked` are withheld, as the current fiber's signal. The caller builds
    /// the frame, which parks as for any `Emit`: the resume value answers the
    /// emit.
    ///
    /// The emitted value is not raised, so it takes no `EmitEscape` retain and
    /// its body releases it as before. The denial struct takes the
    /// `SuspendEscape` retain the resumer's read consumes, as a denied call's
    /// does, and the ledger records it as a payload the runtime built
    /// (docs/impl/region/park.md).
    pub(crate) fn park_denied_emit(&mut self, bits: SignalBits, blocked: SignalBits, value: Value) {
        let payload = {
            let mut ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *self.heap_ptr });
            Self::build_emit_denial_payload(&mut ctx, bits, blocked, value, self.symbols())
        };
        let heap = unsafe { &mut *self.heap_ptr };
        let region = crate::value::arena::region_of(heap, payload);
        crate::value::arena::incref_for_escape(
            heap,
            region,
            crate::value::arena::EscapeSite::SuspendEscape,
        );
        self.fiber.delivery.park_emit_denial(blocked, payload);
        self.fiber.signal = Some((blocked, payload));
    }
}
