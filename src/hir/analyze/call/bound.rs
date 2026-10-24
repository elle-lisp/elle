// audited: 2026-10-24
//! The check of a parameter bound at a call whose argument's signal the
//! compiler knows, made once the unit's signals are solved.
//!
//! docs/signals/inference.md

use super::*;

/// An argument a bound at a call checks: a lambda literal, or a binding.
#[derive(Debug, Clone, Copy)]
pub(in crate::hir::analyze) enum BoundArg {
    Lambda(crate::hir::HirId),
    Binding(Binding),
}

/// A call whose callee bounds a parameter, and the argument each bound checks.
pub(in crate::hir::analyze) struct BoundCall {
    span: Span,
    checks: Vec<(ParamBoundAt, BoundArg)>,
}

impl BoundCall {
    /// Each binding an argument of the call names.
    pub(in crate::hir::analyze) fn bindings(&self) -> impl Iterator<Item = Binding> + '_ {
        self.checks.iter().filter_map(|(_, arg)| match arg {
            BoundArg::Binding(b) => Some(*b),
            BoundArg::Lambda(_) => None,
        })
    }
}

impl<'a> Analyzer<'a> {
    /// Record each argument that a bound the callee declares checks, when
    /// the argument is a lambda literal or a binding. Its signal is final only
    /// once the unit is solved, so `check_bound_calls` checks it then.
    pub(super) fn record_bound_call(&mut self, callee: &Hir, args: &[CallArg], span: Span) {
        let bounds = match &callee.kind {
            HirKind::Lambda(lambda) => {
                Self::bounds_by_position(&lambda.params, &lambda.param_bounds)
            }
            HirKind::Var(binding) => match self.bound_env.get(binding) {
                Some(bounds) => bounds.clone(),
                None => return,
            },
            _ => return,
        };
        let checks: Vec<_> = bounds
            .into_iter()
            .filter_map(|bound| {
                let arg = &args.get(bound.position)?.expr;
                let arg = match &arg.kind {
                    HirKind::Lambda(_) => BoundArg::Lambda(arg.id),
                    HirKind::Var(binding) => BoundArg::Binding(*binding),
                    _ => return None,
                };
                Some((bound, arg))
            })
            .collect();
        if !checks.is_empty() {
            self.bound_calls.push(BoundCall { span, checks });
        }
    }

    /// Check each recorded argument whose signal is known against the bound
    /// the callee declares for its parameter. `known` answers an argument's
    /// final signal. A violation does not compile; an argument whose signal
    /// is not known waits for the callee's entry check.
    pub(in crate::hir::analyze) fn check_bound_calls(
        &self,
        known: impl Fn(BoundArg) -> Option<Signal>,
    ) -> Result<(), String> {
        for call in &self.bound_calls {
            for (bound, arg) in &call.checks {
                let Some(signal) = known(*arg) else {
                    continue;
                };
                let span = call.span;
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
        }
        Ok(())
    }

    /// The signal of a binding the compiler knows for certain as the pass left
    /// it: a primitive's, or the inferred signal of a binding that is never
    /// assigned. Anything else could hold another value by the call.
    pub(in crate::hir::analyze) fn known_binding_signal(&self, binding: Binding) -> Option<Signal> {
        let inner = self.arena.get(binding);
        if inner.is_mutated || !inner.is_immutable {
            return None;
        }
        self.signal_env
            .get(&binding)
            .copied()
            .or_else(|| self.primitive_signal_of(binding))
    }
}
