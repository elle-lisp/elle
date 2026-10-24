// audited: 2026-10-24
//! Raise a unit's HIR to the solver's model: each call, each node above it, and each lambda.
//!
//! docs/impl/solver.md

use super::extract::CallSite;
use super::fixpoint::{bit_values, Bits, Solved};
use super::model::{to_word, FileId, Model, VarId, VarKey};
use crate::hir::{Hir, HirId, HirKind};
use crate::signals::{Signal, CAP_MASK};
use crate::value::fiber::SignalBits;
use std::collections::HashMap;

/// What the model answers for a unit's calls and lambdas.
pub struct Answers<'a> {
    model: &'a Model,
    solved: &'a Solved,
    calls: &'a HashMap<HirId, CallSite>,
    unit: FileId,
}

/// A bit-value set as a signal-bit word.
fn signal_bits(bits: Bits) -> SignalBits {
    SignalBits::new(to_word(bit_values(bits)))
}

impl<'a> Answers<'a> {
    pub fn new(
        model: &'a Model,
        solved: &'a Solved,
        calls: &'a HashMap<HirId, CallSite>,
        unit: FileId,
    ) -> Self {
        Answers {
            model,
            solved,
            calls,
            unit,
        }
    }

    /// Parameter `i` of `owner`, when `w` is one.
    fn param_of(&self, w: VarId, owner: VarId) -> Option<u32> {
        match self.model.keys_by_id[w as usize] {
            VarKey::Param(o, i) if o == owner && i < 32 => Some(i),
            _ => None,
        }
    }

    /// The signal a call of `v` raises, as a function's signal: each
    /// parameter of the function a call binds propagates, and any other free
    /// variable may raise anything.
    pub fn signal_of(&self, v: VarId) -> Signal {
        let owner = self.model.owner(v);
        let mut sig = Signal::of(signal_bits(self.solved.bits(v)));
        for w in self.solved.deps(v) {
            match self.param_of(w, owner) {
                Some(i) => sig.propagates |= 1 << i,
                None => sig.bits = sig.bits.union(CAP_MASK),
            }
        }
        sig
    }

    /// The bits the model answers for the call node `id`. A free variable
    /// that is a parameter of the enclosing lambda adds `:yield` and
    /// `:error`, as the pass's call to a parameter does; any other adds every
    /// bit.
    fn call_bits(&self, id: HirId) -> SignalBits {
        let Some(call) = self.calls.get(&id) else {
            return SignalBits::EMPTY;
        };
        let (raw, rdep) = self.solved.gather(call.ctx, Some(call.site));
        let mut bits = signal_bits(raw);
        for w in rdep {
            bits = bits.union(match self.param_of(w, call.ctx) {
                Some(_) => Signal::yields_errors().bits,
                None => CAP_MASK,
            });
        }
        bits
    }

    /// Raise `hir` and everything under it. A call takes the model's answer,
    /// any other node the bits of each child but a lambda, and a lambda the
    /// model's signal for it. Nothing is lowered.
    pub fn raise(&self, hir: &mut Hir) {
        hir.for_each_child_mut(|c| self.raise(c));
        let id = hir.id;
        if let HirKind::Lambda(lambda) = &mut hir.kind {
            if let Some(v) = self.model.find(&VarKey::Lam(self.unit, id.0)) {
                lambda.inferred_signals = lambda.inferred_signals.combine(self.signal_of(v));
            }
            return;
        }
        let mut bits = hir.signal.bits;
        if matches!(hir.kind, HirKind::Call { .. }) {
            bits = bits.union(self.call_bits(id));
        }
        hir.for_each_child(|c| {
            if !matches!(c.kind, HirKind::Lambda(_)) {
                bits = bits.union(c.signal.bits);
            }
        });
        hir.signal.bits = bits;
    }
}
