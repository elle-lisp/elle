// audited: 2026-10-05
//! The signal of a closure `squelch` or `attune` wraps, when the call names
//! its spec as a literal.
//!
//! docs/signals/inference.md

use super::*;

impl<'a> Analyzer<'a> {
    /// Record in `last_squelch_signal` the signal of the closure a call of
    /// `func` on `args` returns, when `func` is `squelch` or `attune` and the
    /// spec, which comes first, is a literal. The binding analysis seeds the
    /// binding's `signal_env` entry from it. Anything else records `None`.
    pub(super) fn record_squelch_signal(&mut self, func: &Hir, args: &[CallArg]) {
        self.last_squelch_signal = None;
        if args.len() != 2 {
            return;
        }
        let blocked = if self.is_primitive_named(func, "squelch") {
            crate::hir::spec::literal_signal_bits(&args[0].expr, self.arena)
        } else if self.is_primitive_named(func, "attune") {
            crate::hir::spec::literal_signal_bits(&args[0].expr, self.arena)
                .map(|permitted| crate::signals::CAP_MASK.subtract(permitted))
        } else {
            return;
        };
        if let Some(mask) = blocked {
            let target = self.resolve_arg_signal(&args[1].expr);
            self.last_squelch_signal = Some(target.squelch(mask));
        }
    }
}
