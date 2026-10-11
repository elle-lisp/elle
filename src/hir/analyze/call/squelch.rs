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
            self.resolve_squelch_mask(&args[0].expr)
        } else if self.is_primitive_named(func, "attune") {
            self.resolve_squelch_mask(&args[0].expr)
                .map(|permitted| crate::signals::CAP_MASK.subtract(permitted))
        } else {
            return;
        };
        if let Some(mask) = blocked {
            let target = self.resolve_arg_signal(&args[1].expr);
            self.last_squelch_signal = Some(target.squelch(mask));
        }
    }

    /// Resolve a compile-time squelch mask from a keyword or set literal.
    ///
    /// Returns `Some(mask)` for:
    /// - `:keyword` → single signal bit
    /// - `|:kw1 :kw2|` → union of signal bits (set literal in HIR)
    ///
    /// Returns `None` for dynamic values.
    fn resolve_squelch_mask(&self, arg: &Hir) -> Option<crate::value::fiber::SignalBits> {
        use crate::value::fiber::SignalBits;
        match &arg.kind {
            HirKind::Keyword(kw) => {
                let reg = crate::signals::registry::global_registry().lock().unwrap();
                reg.lookup(kw).map(SignalBits::from_bit)
            }
            // Set literal: Call to the `set` primitive with keyword args
            HirKind::Call { func, args, .. } => {
                if let HirKind::Var(binding) = &func.kind {
                    let name = self.symbols.name(self.arena.get(*binding).name)?;
                    if name != "set" {
                        return None;
                    }
                    let reg = crate::signals::registry::global_registry().lock().unwrap();
                    let mut mask = SignalBits::EMPTY;
                    for call_arg in args {
                        if let HirKind::Keyword(kw) = &call_arg.expr.kind {
                            mask = mask.union(SignalBits::from_bit(reg.lookup(kw)?));
                        } else {
                            return None;
                        }
                    }
                    Some(mask)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
