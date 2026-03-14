//! Call analysis and signal tracking

use super::*;
use crate::hir::expr::CallArg;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_call(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        let func = self.analyze_expr(&items[0])?;
        let mut signal = func.signal;

        let mut args = Vec::new();
        let mut has_splice = false;
        for arg in &items[1..] {
            let (inner, spliced) = Self::unwrap_splice(arg);
            let hir = self.analyze_expr(inner)?;
            signal = signal.combine(hir.signal);
            if spliced {
                has_splice = true;
            }
            args.push(CallArg { expr: hir, spliced });
        }

        // Compile-time arity checking — skip when splice makes count unknown
        if !has_splice {
            if let Some(arity) = self.get_callee_arity(&func) {
                let arg_count = args.len();
                if !arity.matches(arg_count) {
                    return Err(format!(
                        "{}: arity error: {} expects {} argument{}, got {}",
                        span,
                        self.callee_name(&func),
                        arity,
                        if arity == Arity::Exact(1) { "" } else { "s" },
                        arg_count,
                    ));
                }
            }
        }

        // Interprocedural signal tracking: what signal does CALLING this function have?
        // First, get the raw callee signal (before polymorphic resolution)
        let raw_callee_signal = self.get_raw_callee_signal(&func);

        // Refine fiber/signal signal when first arg is a constant integer.
        // fiber/signal's registered signal is yields_errors() (conservative),
        // but when the signal bits are known at compile time, use them directly.
        let raw_callee_signal = if self.is_emit(&func) {
            if let Some(HirKind::Int(bits)) = args.first().map(|a| &a.expr.kind) {
                Signal {
                    bits: crate::value::fiber::SignalBits(*bits as u32),
                    propagates: 0,
                }
            } else {
                raw_callee_signal
            }
        } else {
            raw_callee_signal
        };

        // Track signal sources for polymorphic inference BEFORE resolving
        // This handles the case where we call a polymorphic function with a parameter
        let arg_exprs: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
        self.track_signal_source_with_args(&func, &raw_callee_signal, &arg_exprs);

        // Now resolve the polymorphic signal
        let callee_signal = self.resolve_polymorphic_signal(&raw_callee_signal, &arg_exprs);

        signal = signal.combine(callee_signal);

        Ok(Hir::new(
            HirKind::Call {
                func: Box::new(func),
                args,
                is_tail: false, // Tail call marking done in a later pass
            },
            span,
            signal,
        ))
    }

    /// Check if a syntax node is a splice form (`;expr` or `(splice expr)`).
    /// Returns the inner expression and whether it was spliced.
    pub(crate) fn unwrap_splice(syntax: &Syntax) -> (&Syntax, bool) {
        match &syntax.kind {
            SyntaxKind::Splice(inner) => (inner, true),
            SyntaxKind::List(items) if items.len() == 2 => {
                if items[0].as_symbol() == Some("splice") {
                    (&items[1], true)
                } else {
                    (syntax, false)
                }
            }
            _ => (syntax, false),
        }
    }

    /// Get the callee's known arity, if available.
    fn get_callee_arity(&self, callee: &Hir) -> Option<Arity> {
        match &callee.kind {
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                ..
            } => Some(Arity::for_lambda(
                rest_param.is_some(),
                *num_required,
                params.len(),
            )),
            HirKind::Var(binding) => {
                // Check arity env (covers both user-defined and primitive bindings).
                // bind_primitives populates this for primitive bindings; user
                // shadows create new bindings that won't be in arity_env,
                // correctly disabling the primitive arity check.
                self.arity_env.get(binding).copied()
            }
            _ => None,
        }
    }

    /// Get a human-readable name for the callee (for error messages).
    fn callee_name(&self, callee: &Hir) -> String {
        match &callee.kind {
            HirKind::Var(binding) => {
                if let Some(name) = self.symbols.name(binding.name()) {
                    return name.to_string();
                }
                "<unknown>".to_string()
            }
            HirKind::Lambda { .. } => "<lambda>".to_string(),
            _ => "<expression>".to_string(),
        }
    }

    /// Check if the callee is the `emit` primitive.
    fn is_emit(&self, func: &Hir) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            self.symbols.name(binding.name()) == Some("emit")
        } else {
            false
        }
    }

    /// Get the raw callee signal without resolving polymorphic signals.
    pub(crate) fn get_raw_callee_signal(&self, func: &Hir) -> Signal {
        match &func.kind {
            HirKind::Lambda {
                inferred_signals, ..
            } => *inferred_signals,
            HirKind::Var(binding) => {
                if let Some(signal) = self.signal_env.get(binding) {
                    *signal
                } else {
                    self.primitive_signals
                        .get(&binding.name())
                        .cloned()
                        .unwrap_or(Signal::yields())
                }
            }
            _ => Signal::yields(),
        }
    }

    /// Track the source of a suspending signal for polymorphic inference.
    /// This handles both direct parameter calls and calls to polymorphic functions
    /// with parameters as arguments.
    pub(crate) fn track_signal_source_with_args(
        &mut self,
        func: &Hir,
        raw_signal: &Signal,
        args: &[&Hir],
    ) {
        // Case 1: Direct call to a parameter
        if let HirKind::Var(binding) = &func.kind {
            if matches!(binding.scope(), BindingScope::Parameter)
                && self.current_lambda_params.contains(binding)
            {
                self.current_signal_sources.param_calls.insert(*binding);
                return;
            }
        }

        // Case 2: Call to a polymorphic function with parameters as the polymorphic arguments
        if raw_signal.is_polymorphic() {
            let mut found_param = false;
            for param_idx in raw_signal.propagated_params() {
                if param_idx < args.len() {
                    if let HirKind::Var(arg_binding) = &args[param_idx].kind {
                        if matches!(arg_binding.scope(), BindingScope::Parameter)
                            && self.current_lambda_params.contains(arg_binding)
                        {
                            // The polymorphic signal depends on a parameter
                            self.current_signal_sources.param_calls.insert(*arg_binding);
                            found_param = true;
                        }
                    }
                }
            }
            if found_param {
                return;
            }
        }

        // Case 3: Suspension from a non-parameter source
        // Only mark as non-param yield if the resolved signal may suspend
        let resolved_signal = self.resolve_polymorphic_signal(raw_signal, args);
        if resolved_signal.may_suspend() {
            self.current_signal_sources.has_non_param_yield = true;
        }
    }

    /// Resolve a polymorphic signal by examining the arguments at the specified indices.
    pub(crate) fn resolve_polymorphic_signal(&self, signal: &Signal, args: &[&Hir]) -> Signal {
        if signal.is_polymorphic() {
            let mut resolved = Signal::inert();
            for param_idx in signal.propagated_params() {
                if param_idx < args.len() {
                    resolved = resolved.combine(self.resolve_arg_signal(args[param_idx]));
                } else {
                    // Parameter index out of bounds - conservatively Yields
                    return Signal::yields();
                }
            }
            resolved
        } else {
            *signal
        }
    }

    /// Resolve the signal of an argument (used for polymorphic signal resolution).
    /// When the polymorphic parameter is itself a lambda or known function,
    /// we can determine its signal.
    pub(crate) fn resolve_arg_signal(&self, arg: &Hir) -> Signal {
        match &arg.kind {
            HirKind::Lambda {
                inferred_signals, ..
            } => *inferred_signals,
            HirKind::Var(binding) => self
                .signal_env
                .get(binding)
                .cloned()
                .or_else(|| self.primitive_signals.get(&binding.name()).cloned())
                .unwrap_or(Signal::yields()),
            // Unknown argument signal - conservatively Yields for soundness
            _ => Signal::yields(),
        }
    }

    /// Compute the inferred signal for a lambda based on signal sources.
    /// This enables polymorphic signal inference: if the only sources of
    /// suspension are calling parameters, we infer Polymorphic over them.
    pub(crate) fn compute_inferred_signal(&self, body: &Hir, params: &[Binding]) -> Signal {
        // If body doesn't suspend, lambda doesn't suspend
        if !body.signal.may_suspend() {
            return Signal::inert();
        }

        // If there's a direct yield or non-parameter yield, it's Yields
        if self.current_signal_sources.has_direct_yield
            || self.current_signal_sources.has_non_param_yield
        {
            return Signal::yields();
        }

        // If param_calls is empty but body suspends, fall back to body's signal
        if self.current_signal_sources.param_calls.is_empty() {
            return body.signal;
        }

        // All suspension comes from parameter calls - infer Polymorphic over them.
        // Bounded parameters contribute their bound's bits directly (not polymorphic).
        let mut propagates: u32 = 0;
        let mut bound_bits: u32 = 0;
        for binding_id in &self.current_signal_sources.param_calls {
            if let Some(bound) = self.current_param_bounds.get(binding_id) {
                // Bounded: contribute bound's bits directly (not polymorphic)
                bound_bits |= bound.bits.0;
            } else if let Some(idx) = params.iter().position(|p| p == binding_id) {
                // Unbounded: polymorphic propagation
                propagates |= 1 << idx;
            }
        }

        Signal {
            bits: crate::value::fiber::SignalBits(bound_bits),
            propagates,
        }
    }
}
