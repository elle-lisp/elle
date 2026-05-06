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

        // Refine emit's signal when first arg is known at compile time.
        // emit's registered signal is yields_errors() (conservative), but:
        // - When the signal bits are a constant integer, use them directly.
        // - When the signal is a keyword registered in the signal registry,
        //   look up its bit position and use that.
        // This enables accurate signal inference for (emit :kw value) forms
        // where the signal keyword was declared with (signal :kw) earlier.
        let raw_callee_signal = if self.is_emit(&func) {
            if let Some(first_arg_kind) = args.first().map(|a| &a.expr.kind) {
                match first_arg_kind {
                    HirKind::Int(bits) => Signal {
                        bits: crate::value::fiber::SignalBits::from_i64(*bits),
                        propagates: 0,
                    },
                    HirKind::Keyword(kw) => {
                        let reg = crate::signals::registry::global_registry().lock().unwrap();
                        if let Some(bit_pos) = reg.lookup(kw) {
                            // User signals are emitted via the yield mechanism.
                            // Include SIG_YIELD so may_suspend() returns true,
                            // enabling correct signal inference for the enclosing lambda.
                            // Also include the specific user signal bit for accurate
                            // squelch checking at the static type level.
                            Signal {
                                bits: crate::value::fiber::SignalBits::from_bit(bit_pos)
                                    .union(crate::signals::SIG_YIELD),
                                propagates: 0,
                            }
                        } else {
                            raw_callee_signal
                        }
                    }
                    _ => raw_callee_signal,
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

        // ── Import projection detection ────────────────────────────────
        // Pattern: ((import "literal")) — the outer call's func is itself
        // a Call to `import` with a literal string argument. If so, look up
        // the target file's signal projection and stash it for the binding
        // analysis to pick up via `last_import_projection`.
        self.last_import_projection = None;
        self.last_import_escape_projection = None;
        if let HirKind::Call {
            func: inner_func,
            args: inner_args,
            ..
        } = &func.kind
        {
            if self.is_import(inner_func) {
                if let Some(first) = inner_args.first() {
                    if let HirKind::String(spec) = &first.expr.kind {
                        if let Some(resolved) = crate::primitives::modules::resolve_import(spec) {
                            self.last_import_projection =
                                crate::pipeline::get_or_compile_projection(&resolved);
                            self.last_import_escape_projection =
                                crate::pipeline::get_or_compile_escape_projection(&resolved);
                        }
                    }
                }
            }
        }

        // ── Compile-time squelch/attune detection ─────────────────────
        // Pattern: (squelch f :keyword) or (squelch f |:kw1 :kw2|)
        //          (attune f :keyword) or (attune f |:kw1 :kw2|)
        // Compute the resulting closure's signal statically and stash it
        // for binding analysis to seed the binding's signal_env entry.
        self.last_squelch_signal = None;
        if self.is_squelch(&func) && args.len() == 2 {
            let target_signal = self.resolve_arg_signal(&args[0].expr);
            if let Some(mask) = self.resolve_squelch_mask(&args[1].expr) {
                self.last_squelch_signal = Some(target_signal.squelch(mask));
            }
        } else if self.is_attune(&func) && args.len() == 2 {
            // attune is mask-first: (attune |:yield| closure)
            let target_signal = self.resolve_arg_signal(&args[1].expr);
            if let Some(permitted) = self.resolve_squelch_mask(&args[0].expr) {
                // attune permits only these bits; suppress everything else.
                let suppress = crate::signals::CAP_MASK.subtract(permitted);
                self.last_squelch_signal = Some(target_signal.squelch(suppress));
            }
        }

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
                if let Some(name) = self.symbols.name(self.arena.get(*binding).name) {
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
        self.is_primitive_named(func, "emit")
    }

    /// Check if the callee is the `squelch` primitive.
    fn is_squelch(&self, func: &Hir) -> bool {
        self.is_primitive_named(func, "squelch")
    }

    /// Check if the callee is the `attune` primitive.
    fn is_attune(&self, func: &Hir) -> bool {
        self.is_primitive_named(func, "attune")
    }

    /// Check if the callee is the `import` primitive.
    fn is_import(&self, func: &Hir) -> bool {
        self.is_primitive_named(func, "import")
    }

    /// Check if a callee HIR node refers to a named primitive.
    fn is_primitive_named(&self, func: &Hir, name: &str) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            self.symbols.name(self.arena.get(*binding).name) == Some(name)
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
                } else if let Some(signal) = self
                    .primitive_signals
                    .get(&self.arena.get(*binding).name)
                    .cloned()
                {
                    signal
                } else if matches!(self.arena.get(*binding).scope, BindingScope::Parameter)
                    && self.current_lambda_params.contains(binding)
                {
                    // Parameter call: signal depends on what the caller passes.
                    // SIG_YIELD triggers the polymorphic inference path in
                    // compute_inferred_signal; SIG_ERROR is inherently sound
                    // because calling an unknown value can always fail (not
                    // callable, wrong arity, callback errors).
                    Signal::yields_errors()
                } else {
                    // Calling a value whose origin is opaque to static analysis
                    // (e.g., bound to a dynamic expression result). We cannot
                    // determine its effects — use the sound conservative signal.
                    Signal::unknown()
                }
            }
            // Opaque expression in callee position (result of a call, conditional,
            // etc.) — effects are statically indeterminate.
            _ => Signal::unknown(),
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
            if matches!(self.arena.get(*binding).scope, BindingScope::Parameter)
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
                        if matches!(self.arena.get(*arg_binding).scope, BindingScope::Parameter)
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

        // Case 3: Signal from a non-parameter callee — inherent to this function.
        let resolved_signal = self.resolve_polymorphic_signal(raw_signal, args);
        self.current_signal_sources.non_param_bits = self
            .current_signal_sources
            .non_param_bits
            .union(resolved_signal.bits);
    }

    /// Resolve a polymorphic signal by examining the arguments at the specified indices.
    /// Preserves inherent bits (non-polymorphic signals) and combines with resolved args.
    pub(crate) fn resolve_polymorphic_signal(&self, signal: &Signal, args: &[&Hir]) -> Signal {
        if signal.is_polymorphic() {
            // Start with inherent bits (signals that don't depend on parameters)
            let mut resolved = Signal {
                bits: signal.bits,
                propagates: 0,
            };
            for param_idx in signal.propagated_params() {
                if param_idx < args.len() {
                    resolved = resolved.combine(self.resolve_arg_signal(args[param_idx]));
                } else {
                    // Parameter index out of bounds — sound conservative signal.
                    return Signal::unknown();
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
                .or_else(|| {
                    self.primitive_signals
                        .get(&self.arena.get(*binding).name)
                        .cloned()
                })
                .unwrap_or(Signal::unknown()),
            // Opaque expression as argument — effects are indeterminate.
            _ => Signal::unknown(),
        }
    }

    /// Compute the inferred signal for a lambda based on signal sources.
    /// This enables polymorphic signal inference: if the only sources of
    /// suspension are calling parameters, we infer Polymorphic over them.
    pub(crate) fn compute_inferred_signal(&self, body: &Hir, params: &[Binding]) -> Signal {
        // Silent body → silent function.
        if body.signal.bits.is_empty() && body.signal.propagates == 0 {
            return Signal::silent();
        }

        // Inherent bits: from direct emits and non-parameter callees.
        let inherent = self
            .current_signal_sources
            .direct_bits
            .union(self.current_signal_sources.non_param_bits);

        // If no parameter calls contribute signals, the function's
        // signal is fully determined by its inherent bits.
        if self.current_signal_sources.param_calls.is_empty() {
            return Signal {
                bits: inherent,
                propagates: 0,
            };
        }

        // Build polymorphic propagation from parameter calls.
        // Silence-bounded parameters contribute their bound's bits
        // to inherent (not polymorphic).
        let mut propagates: u32 = 0;
        let mut bound_bits = crate::value::fiber::SignalBits::EMPTY;
        for binding_id in &self.current_signal_sources.param_calls {
            if let Some(bound) = self.current_param_bounds.get(binding_id) {
                bound_bits = bound_bits.union(bound.bits);
            } else if let Some(idx) = params.iter().position(|p| p == binding_id) {
                propagates |= 1 << idx;
            }
        }

        Signal {
            bits: inherent.union(bound_bits),
            propagates,
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
