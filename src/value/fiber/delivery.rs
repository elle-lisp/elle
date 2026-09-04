//! The delivery ledger: how the current park's delivery references are funded.
//!
//! Every value that crosses a fiber boundary carries exactly one delivery
//! reference, minted by the crossing and consumed by exactly one reader on the
//! other side. Which side mints, and what else the park owes, depends on the
//! park's shape — and only the site that builds the park knows the shape. The
//! ledger is the one record that carries the answer from the park to the seam
//! that ends it (docs/impl/region/owner.md § "A park names its funding in the
//! delivery ledger").
//!
//! The fields are private: a park names its funding through a method or not at
//! all, and each method carries the invariant its transition upholds. The
//! ledger is uncounted throughout — the payloads it names are only ever
//! compared bit-wise, never dereferenced, so it takes no retain and the Fiber
//! content scan records no edge for them. The counted edge for the same value
//! is `Fiber::signal`'s.

use crate::value::{SignalBits, Value};

/// The funding record of a fiber's current park. One instance rides each
/// `Fiber` (`Fiber::delivery`); see the module doc for the model and
/// docs/impl/region/owner.md for the per-shape rules the methods encode.
#[derive(Default)]
pub struct Delivery {
    /// The parked `SIG_ERROR` payload whose delivery reference the raise or
    /// injection minted, rather than this fiber's frames: an `emit` raise
    /// (`EmitEscape`, taken by `handle_emit` and its JIT mirror), the same
    /// raise leaving the emit primitive in either position
    /// (`VM::mint_raised_argument_delivery`), or an injected `fiber/abort` /
    /// `fiber/refuse` payload (`AbortDelivery`). While this names the live
    /// signal's payload, the payload exemption on the abandoned-frame walk and
    /// the parked frame's discharge is withdrawn: a frame's own reference funds
    /// no delivery, so every release the tables name is genuinely owed
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    /// it still owes"). A native install records nothing here — its delivery is
    /// the payload's birth reference or the frame's left-standing one, which
    /// the exemption preserves.
    minted: Option<Value>,
    /// The capability-denial payload parked in `Fiber::signal`, whose region
    /// the install that displaces it owes one decref. The denial path builds
    /// the `{:error :capability-denied …}` struct itself, so the body never
    /// names it and no `decref_point` names its region; the reference the
    /// allocation left is owed by whatever replaces the payload in the slot —
    /// a resume's delivery, or an abort's / refusal's injected error
    /// (docs/impl/region/owner.md § "Park/unpark symmetry" — "A payload the
    /// RUNTIME built is released by the install that displaces it"). Carried
    /// as the payload rather than a flag so the release is gated on
    /// representation identity with the live parked signal, and TAKEN by the
    /// displacing install ([`Self::take_bodyless`]) so no second install can
    /// release the same reference.
    bodyless: Option<Value>,
    /// The live park's payload and bits, whose DELIVERY reference — the escape
    /// retain the park took as the value went into `Fiber::signal` — no reader
    /// has consumed. A resume consumes it: the resumer's compiler-emitted
    /// release of the resume result is that reader, so [`Self::take_resume_funding`]
    /// clears the record at the crossing. A `squelch`/`attune` boundary is the
    /// one end of a park that has no reader at all, and it releases what this
    /// names (docs/impl/region/owner.md § "A boundary ends a park with no reader
    /// and no install").
    ///
    /// Written by the site that TAKES the retain — the two suspend arms, the two
    /// denial arms, the `Emit` handler, and their JIT twins — so a record exists
    /// exactly where a retain does and names the value it was taken on. That is
    /// what the boundary cannot read out of `Fiber::signal`: two enforcement
    /// sites reach it through `invoke_closure_jit`, which restores the CALLER's
    /// signal first and holds the parked one in a local.
    ///
    /// An IMMEDIATE payload records nothing: `incref_for_escape` is a no-op for a
    /// value living in no region, so such a park took no retain and there is no
    /// reference for any seam to consume. Written through [`Self::record_park`],
    /// which is where that gate lives.
    ///
    /// Payload-named like [`Self::bodyless`], and gated the same way: the consumer
    /// compares it bit-wise against the signal it is looking at, so a record left
    /// over from a park some other route ended names nothing and is overwritten by
    /// the next park rather than needing a clear on every route.
    ///
    /// Uncounted like every other field here: the payload is compared bit-wise
    /// and its region resolved, never structurally read, and the counted edge for
    /// the same value is `Fiber::signal`'s.
    undelivered: Option<(SignalBits, Value)>,
    /// Whether this fiber's innermost suspension is a PRIMITIVE call, whose
    /// resume value therefore arrives owing one reference. A parked frame
    /// re-enters at its suspending call's continuation, which runs that call's
    /// compiler-emitted result release; a bytecode callee funds that reference
    /// with its `Return` mint, but a primitive that suspends never returns, so
    /// the delivery mints it instead (docs/impl/region/owner.md § "A delivery
    /// into a replayed frame carries one owning reference"). Rides the fiber
    /// rather than the frame because a tail suspend's park is built later and
    /// elsewhere, by a driver that never saw the primitive.
    resume_unfunded: bool,
}

impl Delivery {
    /// A fresh ledger: no park, nothing owed.
    pub fn new() -> Self {
        Self::default()
    }

    /// A suspending PRIMITIVE parked (a yielding io op, a dynamic `emit`): the
    /// resume value owes one `ResumeDelivery` mint at the delivery funnel.
    /// Callers: `handle_primitive_signal[_tail]`'s Suspend arms and their JIT
    /// twin (`jit_handle_primitive_signal`).
    pub(crate) fn park_primitive(&mut self, bits: SignalBits, payload: Value) {
        self.assert_consumed();
        self.resume_unfunded = true;
        self.record_park(bits, payload);
    }

    /// An `Emit` node parked (`handle_emit` and its JIT twin `elle_jit_yield`).
    /// The compiler funds this continuation itself — the emit's own decref_point
    /// balances the release past the suspend — so the park owes no resume mint,
    /// and the record is the escape retain's alone. Never reached by a TERMINAL
    /// emit: an error raise parks nothing and records its mint instead, and a
    /// halt takes no retain at all.
    pub(crate) fn park_emit(&mut self, bits: SignalBits, payload: Value) {
        self.assert_consumed();
        self.record_park(bits, payload);
    }

    /// A capability denial parked: a primitive park (the denied call never
    /// returns) whose payload also has no body reference, so the resume owes
    /// its region one decref besides the mint. Callers:
    /// `handle_capability_denial[_tail]` and `jit_capability_denial`.
    pub(crate) fn park_denial(&mut self, bits: SignalBits, payload: Value) {
        self.assert_consumed();
        self.resume_unfunded = true;
        self.bodyless = Some(payload);
        self.record_park(bits, payload);
    }

    /// Record the park whose escape retain no reader has consumed, where there
    /// was a retain to take. An immediate payload lives in no region, so
    /// `incref_for_escape` did nothing at the park and there is nothing here for
    /// a boundary to release — recording one would leave [`Self::assert_consumed`]
    /// asserting over a park that owes no seam anything.
    fn record_park(&mut self, bits: SignalBits, payload: Value) {
        self.undelivered = payload.as_heap_ptr().is_some().then_some((bits, payload));
    }

    /// The net for a park shape wired without a consume seam: every route out
    /// of a park passes [`Self::take_resume_funding`], [`Self::install_abort`],
    /// or [`Self::discharge`], so a park write finding the previous park's
    /// funding still standing means some route skipped all three — one region
    /// leaked per cycle in release builds, a panic here.
    ///
    /// [`Self::undelivered`] is deliberately outside the net, exactly as
    /// [`Self::bodyless`] is: both are payload-named records, gated at their
    /// consumers by bit-wise identity with the signal the consumer is looking
    /// at, so a stale one names nothing and is overwritten by the next park.
    #[track_caller]
    fn assert_consumed(&self) {
        debug_assert!(
            !self.resume_unfunded,
            "delivery ledger: parking over an unconsumed park — a route ended \
             the previous park without consuming its resume funding \
             (docs/impl/region/owner.md § \"A park names its funding in the \
             delivery ledger\")",
        );
    }

    /// A raise minted the parked payload's delivery itself, so the frames'
    /// owed releases run in full. Callers: `handle_emit` and its JIT mirror,
    /// `VM::mint_raised_argument_delivery`, and `VM::park_propagating_abort`
    /// (the mint there is the injection's, travelling with the payload).
    pub(crate) fn record_mint(&mut self, payload: Value) {
        self.minted = Some(payload);
    }

    /// An abort injection installed its payload over the park: the injection's
    /// mint is recorded, the displaced park's payload records leave with its
    /// payload, and no resume value is owed — an abort delivers none (the
    /// replayed frame re-enters with `SIG_ERROR` set and leaves before the
    /// parked call's result release). Caller: `do_fiber_abort`.
    pub(crate) fn install_abort(&mut self, payload: Value) {
        self.minted = Some(payload);
        self.bodyless = None;
        self.undelivered = None;
        self.resume_unfunded = false;
    }

    /// The delivery funnel takes the park's funding with the parked signal:
    /// clears the mint record (the payload it named leaves the slot) and
    /// answers whether the resume value owes the `ResumeDelivery` mint, so a
    /// later park of a different shape starts from nothing. Callers:
    /// `do_fiber_resume_single` and the WASM tier's `handle_fiber_resume` —
    /// one method for both tiers, so they cannot drift.
    pub(crate) fn take_resume_funding(&mut self) -> bool {
        self.minted = None;
        // The crossing: the resumer read the payload out and its own release of
        // the resume result consumes the escape retain, so the park is delivered
        // and no boundary may claim it again.
        self.undelivered = None;
        std::mem::take(&mut self.resume_unfunded)
    }

    /// An install replaced the parked payload without delivering it (the
    /// trampoline's `FiberResume` short-circuit): the payload-named records
    /// describe a value no longer in the slot, so they go with it. The
    /// displacing installs consume the bodyless record's release through
    /// `release_displaced_denial_payload` ([`Self::take_bodyless`]) before
    /// this runs, so the clear here is the mint record's. `resume_unfunded`
    /// survives — the funnel that consumes it has not run yet, and the
    /// replayed frame still owes its resume value the mint.
    pub(crate) fn displace(&mut self) {
        self.minted = None;
        self.bodyless = None;
        self.undelivered = None;
    }

    /// `Fiber::take_parked_state` consumed the park of a fiber that can never
    /// run again: no funding survives, so a later (invalid) resume of a killed
    /// fiber mints nothing.
    pub(crate) fn discharge(&mut self) {
        self.minted = None;
        self.bodyless = None;
        self.undelivered = None;
        self.resume_unfunded = false;
    }

    /// Whether the raise minted the delivery of exactly this payload
    /// (representation identity, never structural equality) — the gate the
    /// abandoned-frame walk and the parked frame's discharge read to withdraw
    /// the payload exemption. A stale record matches nothing and withholds
    /// nothing.
    pub(crate) fn mint_names(&self, payload: Value) -> bool {
        self.minted.is_some_and(|m| m.bit_identical(payload))
    }

    /// Take the recorded bodyless (denial) payload — the one consuming read,
    /// run by `release_displaced_denial_payload` at every install that
    /// replaces the parked signal. Taking is the receipt: a second install
    /// finds nothing and releases nothing.
    pub(crate) fn take_bodyless(&mut self) -> Option<Value> {
        self.bodyless.take()
    }

    /// Take the live park whose delivery reference no reader consumed — the one
    /// consuming read, run by the `squelch`/`attune` discard chokepoint. Taking
    /// is the receipt, so a second boundary over the same fiber releases nothing.
    /// `None` for a fiber with no live park, and for one whose park already
    /// crossed to a resumer.
    pub(crate) fn take_undelivered(&mut self) -> Option<(SignalBits, Value)> {
        self.undelivered.take()
    }

    /// The recorded bodyless payload, without consuming it — a read for tests;
    /// the consuming read is [`Self::take_bodyless`].
    #[cfg(test)]
    pub(crate) fn bodyless(&self) -> Option<Value> {
        self.bodyless
    }

    /// Whether the next delivery owes the resume value a mint — a read for
    /// tests; the consuming read is [`Self::take_resume_funding`].
    #[cfg(test)]
    pub(crate) fn resume_unfunded(&self) -> bool {
        self.resume_unfunded
    }
}

#[cfg(test)]
mod tests;
