//! Call analysis and effect tracking

use super::*;
use crate::syntax::Syntax;

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_call(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        let func = self.analyze_expr(&items[0])?;
        let mut effect = func.effect;

        let mut args = Vec::new();
        for arg in &items[1..] {
            let hir = self.analyze_expr(arg)?;
            effect = effect.combine(hir.effect);
            args.push(hir);
        }

        // Interprocedural effect tracking: what effect does CALLING this function have?
        // First, get the raw callee effect (before polymorphic resolution)
        let raw_callee_effect = self.get_raw_callee_effect(&func);

        // Track effect sources for polymorphic inference BEFORE resolving
        // This handles the case where we call a polymorphic function with a parameter
        self.track_effect_source_with_args(&func, &raw_callee_effect, &args);

        // Now resolve the polymorphic effect
        let callee_effect = self.resolve_polymorphic_effect(&raw_callee_effect, &args);

        effect = effect.combine(callee_effect);

        Ok(Hir::new(
            HirKind::Call {
                func: Box::new(func),
                args,
                is_tail: false, // Tail call marking done in a later pass
            },
            span,
            effect,
        ))
    }

    /// Get the raw callee effect without resolving polymorphic effects.
    pub(crate) fn get_raw_callee_effect(&self, func: &Hir) -> Effect {
        match &func.kind {
            HirKind::Lambda {
                inferred_effect, ..
            } => *inferred_effect,
            HirKind::Var(binding_id) => {
                if let Some(effect) = self.effect_env.get(binding_id) {
                    *effect
                } else if let Some(info) = self.ctx.get_binding(*binding_id) {
                    if matches!(info.kind, BindingKind::Global) {
                        // Check primitive effects first, then global effects from previous forms
                        self.primitive_effects
                            .get(&info.name)
                            .or_else(|| self.global_effects.get(&info.name))
                            .cloned()
                            .unwrap_or(Effect::yields())
                    } else {
                        Effect::yields()
                    }
                } else {
                    Effect::yields()
                }
            }
            _ => Effect::yields(),
        }
    }

    /// Track the source of a suspending effect for polymorphic inference.
    /// This handles both direct parameter calls and calls to polymorphic functions
    /// with parameters as arguments.
    pub(crate) fn track_effect_source_with_args(
        &mut self,
        func: &Hir,
        raw_effect: &Effect,
        args: &[Hir],
    ) {
        // Case 1: Direct call to a parameter
        if let HirKind::Var(binding_id) = &func.kind {
            if let Some(info) = self.ctx.get_binding(*binding_id) {
                if matches!(info.kind, BindingKind::Parameter { .. })
                    && self.current_lambda_params.contains(binding_id)
                {
                    self.current_effect_sources.param_calls.insert(*binding_id);
                    return;
                }
            }
        }

        // Case 2: Call to a polymorphic function with parameters as the polymorphic arguments
        if raw_effect.is_polymorphic() {
            let mut found_param = false;
            for param_idx in raw_effect.propagated_params() {
                if param_idx < args.len() {
                    if let HirKind::Var(arg_binding_id) = &args[param_idx].kind {
                        if let Some(info) = self.ctx.get_binding(*arg_binding_id) {
                            if matches!(info.kind, BindingKind::Parameter { .. })
                                && self.current_lambda_params.contains(arg_binding_id)
                            {
                                // The polymorphic effect depends on a parameter
                                self.current_effect_sources
                                    .param_calls
                                    .insert(*arg_binding_id);
                                found_param = true;
                            }
                        }
                    }
                }
            }
            if found_param {
                return;
            }
        }

        // Case 3: Suspension from a non-parameter source
        // Only mark as non-param yield if the resolved effect may suspend
        let resolved_effect = self.resolve_polymorphic_effect(raw_effect, args);
        if resolved_effect.may_suspend() {
            self.current_effect_sources.has_non_param_yield = true;
        }
    }

    /// Resolve a polymorphic effect by examining the arguments at the specified indices.
    pub(crate) fn resolve_polymorphic_effect(&self, effect: &Effect, args: &[Hir]) -> Effect {
        if effect.is_polymorphic() {
            let mut resolved = Effect::none();
            for param_idx in effect.propagated_params() {
                if param_idx < args.len() {
                    resolved = resolved.combine(self.resolve_arg_effect(&args[param_idx]));
                } else {
                    // Parameter index out of bounds - conservatively Yields
                    return Effect::yields();
                }
            }
            resolved
        } else {
            *effect
        }
    }

    /// Resolve the effect of an argument (used for polymorphic effect resolution).
    /// When the polymorphic parameter is itself a lambda or known function,
    /// we can determine its effect.
    pub(crate) fn resolve_arg_effect(&self, arg: &Hir) -> Effect {
        match &arg.kind {
            HirKind::Lambda {
                inferred_effect, ..
            } => *inferred_effect,
            HirKind::Var(id) => self
                .effect_env
                .get(id)
                .cloned()
                .or_else(|| {
                    self.ctx
                        .get_binding(*id)
                        .filter(|info| matches!(info.kind, BindingKind::Global))
                        .and_then(|info| {
                            // Check primitive effects first, then global effects from previous forms
                            self.primitive_effects
                                .get(&info.name)
                                .or_else(|| self.global_effects.get(&info.name))
                                .cloned()
                        })
                })
                .unwrap_or(Effect::yields()),
            // Unknown argument effect - conservatively Yields for soundness
            _ => Effect::yields(),
        }
    }

    /// Compute the inferred effect for a lambda based on effect sources.
    /// This enables polymorphic effect inference: if the only sources of
    /// suspension are calling parameters, we infer Polymorphic over them.
    pub(crate) fn compute_inferred_effect(&self, body: &Hir, params: &[BindingId]) -> Effect {
        // If body doesn't suspend, lambda doesn't suspend
        if !body.effect.may_suspend() {
            return Effect::none();
        }

        // If there's a direct yield or non-parameter yield, it's Yields
        if self.current_effect_sources.has_direct_yield
            || self.current_effect_sources.has_non_param_yield
        {
            return Effect::yields();
        }

        // If param_calls is empty but body suspends, fall back to body's effect
        if self.current_effect_sources.param_calls.is_empty() {
            return body.effect;
        }

        // All suspension comes from parameter calls - infer Polymorphic over them
        let mut propagates: u32 = 0;
        for binding_id in &self.current_effect_sources.param_calls {
            if let Some(idx) = params.iter().position(|p| p == binding_id) {
                propagates |= 1 << idx;
            }
        }

        if propagates == 0 {
            Effect::yields() // shouldn't happen
        } else {
            Effect {
                bits: 0,
                propagates,
            }
        }
    }
}
