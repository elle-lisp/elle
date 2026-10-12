// audited: 2026-10-05
//! The check of a parameter bound at a call whose argument's signal the
//! compiler knows.
//!
//! docs/signals/inference.md

use super::*;

impl<'a> Analyzer<'a> {
    /// Check each argument whose signal is known against the bound the callee
    /// declares for its parameter. A violation does not compile; an argument
    /// whose signal is not known waits for the callee's entry check.
    pub(super) fn check_bounds_at_call(
        &self,
        callee: &Hir,
        args: &[CallArg],
        span: Span,
    ) -> Result<(), String> {
        let bounds = match &callee.kind {
            HirKind::Lambda {
                params,
                param_bounds,
                ..
            } => Self::bounds_by_position(params, param_bounds),
            HirKind::Var(binding) => match self.bound_env.get(binding) {
                Some(bounds) => bounds.clone(),
                None => return Ok(()),
            },
            _ => return Ok(()),
        };
        for bound in bounds {
            let Some(arg) = args.get(bound.position) else {
                continue;
            };
            let Some(signal) = self.known_arg_signal(&arg.expr) else {
                continue;
            };
            let name = self
                .symbols
                .name(self.arena.get(bound.param).name)
                .unwrap_or("?");
            let allowed = crate::signals::registry::format_bits(bound.allowed);
            if signal.is_polymorphic() {
                return Err(format!(
                    "{}: bound: argument passes on the signal of its arguments but \
                     parameter {} is bounded to {}",
                    span, name, allowed
                ));
            }
            let excess = signal.bits.subtract(bound.allowed);
            if !excess.is_empty() {
                return Err(format!(
                    "{}: bound: argument may emit {} but parameter {} is bounded to {}",
                    span,
                    crate::signals::registry::format_bits(excess),
                    name,
                    allowed
                ));
            }
        }
        Ok(())
    }

    /// The signal of an argument the compiler knows for certain: a lambda
    /// literal's, a primitive's, or the inferred signal of a binding that is
    /// never assigned. Anything else could hold another value by the call.
    fn known_arg_signal(&self, arg: &Hir) -> Option<Signal> {
        match &arg.kind {
            HirKind::Lambda {
                inferred_signals, ..
            } => Some(*inferred_signals),
            HirKind::Var(binding) => {
                let inner = self.arena.get(*binding);
                if inner.is_mutated || !inner.is_immutable {
                    return None;
                }
                self.signal_env
                    .get(binding)
                    .copied()
                    .or_else(|| self.primitive_signal_of(*binding))
            }
            _ => None,
        }
    }
}
