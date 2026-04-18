//! Binding forms: let, letrec, define, set

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_let(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let requires bindings list", span));
        }

        let bindings_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::ArrayMut(_)) {
                format!(
                    "{}: let bindings must use (...) or [...], not @[...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: let bindings must be a list (...) or [...], got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;

        // Phase 1: Analyze all value expressions in the OUTER scope.
        // For destructuring bindings, we record the pattern syntax for Phase 2.
        // Bindings are flat pairs: [name1 value1 name2 value2 ...]
        enum LetBinding<'s> {
            Simple(&'s str, Vec<ScopeId>, Hir),
            Destructure(&'s Syntax, Hir),
        }
        let mut analyzed = Vec::new();
        let mut signal = Signal::silent();

        if bindings_syntax.len() % 2 != 0 {
            return Err(format!(
                "{}: let bindings must have an even number of forms (name/value pairs)",
                span
            ));
        }

        let mut i = 0;
        while i < bindings_syntax.len() {
            let name_syn = &bindings_syntax[i];
            let value_syn = &bindings_syntax[i + 1];

            let value = self.analyze_expr(value_syn)?;
            signal = signal.combine(value.signal);

            if let Some(name) = name_syn.as_symbol() {
                analyzed.push(LetBinding::Simple(name, name_syn.scopes.clone(), value));
            } else if Self::is_destructure_pattern(name_syn) {
                analyzed.push(LetBinding::Destructure(name_syn, value));
            } else {
                return Err(format!(
                    "{}: let binding name must be a symbol, list, or array",
                    span
                ));
            }
            i += 2;
        }

        // Phase 2: Push scope and create all bindings
        self.push_scope(false);

        let mut bindings = Vec::new();
        let mut destructures = Vec::new();

        for item in analyzed {
            match item {
                LetBinding::Simple(name, name_scopes, value) => {
                    let (actual_name, is_mutable) = super::strip_at_prefix(name);
                    let binding = self.bind(actual_name, &name_scopes, BindingScope::Local);
                    if !is_mutable {
                        self.arena.get_mut(binding).is_immutable = true;
                    }
                    // Track signal and arity for interprocedural analysis
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_signals,
                        ..
                    } = &value.kind
                    {
                        self.signal_env.insert(binding, *inferred_signals);
                        let arity = Arity::for_lambda(
                            rest_param.is_some(),
                            *num_required,
                            lambda_params.len(),
                        );
                        self.arity_env.insert(binding, arity);
                    }
                    bindings.push((binding, value));
                }
                LetBinding::Destructure(pattern_syntax, value) => {
                    // Create a temp binding for the value
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    // Analyze the pattern (creates leaf bindings in this scope)
                    // Immutable by default; individual leaves with @ opt into mutability
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        true,
                        &span,
                    )?;
                    destructures.push((pattern, tmp));
                }
            }
        }

        // Analyze body expressions (empty body returns nil)
        let body = if items.len() > 2 {
            self.analyze_body(&items[2..], span.clone())?
        } else {
            Hir::silent(HirKind::Nil, span.clone())
        };
        signal = signal.combine(body.signal);

        self.pop_scope();

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::silent(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::silent(HirKind::Var(tmp), span.clone())),
                            strict: true,
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), signal)
        };

        Ok(Hir::new(
            HirKind::Let {
                bindings,
                body: Box::new(final_body),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_letrec(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: letrec requires bindings and body", span));
        }

        let bindings_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::ArrayMut(_)) {
                format!(
                    "{}: letrec bindings must use (...) or [...], not @[...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: letrec bindings must be a list (...) or [...], got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;

        self.push_scope(false);

        // Pass 1: Classify each binding. Pre-bind ALL names for mutual
        // visibility — simple symbols AND destructure leaf names.
        // Destructure leaf names are pre-bound so that other initializers
        // (e.g., recursive functions) can reference them.
        //
        // The double-binding problem: analyze_destructure_pattern in pass 2
        // also calls self.bind() for the same names. To prevent creating
        // duplicate Binding objects, analyze_destructure_pattern checks
        // lookup_in_current_scope for the Local scope case, reusing
        // pre-existing bindings.
        //
        // Bindings are flat pairs: [name1 value1 name2 value2 ...]
        enum LetrecEntry<'s> {
            Simple(Binding, &'s Syntax),
            Destructure {
                pattern: &'s Syntax,
                value: &'s Syntax,
                leaf_bindings: HashMap<String, Binding>,
            },
        }
        let mut entries = Vec::new();

        if bindings_syntax.len() % 2 != 0 {
            return Err(format!(
                "{}: letrec bindings must have an even number of forms (name/value pairs)",
                span
            ));
        }

        let mut i = 0;
        while i < bindings_syntax.len() {
            let name_syn = &bindings_syntax[i];
            let value_syn = &bindings_syntax[i + 1];

            if let Some(raw_name) = name_syn.as_symbol() {
                // Simple binding — bind immediately for mutual recursion.
                // Marked prebound: may be captured before initialization.
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let b = self.bind(name, &[], BindingScope::Local);
                self.arena.get_mut(b).is_prebound = true;
                if !is_mutable {
                    self.arena.get_mut(b).is_immutable = true;
                }
                entries.push(LetrecEntry::Simple(b, value_syn));
            } else if Self::is_destructure_pattern(name_syn) {
                // Destructure pattern — pre-bind leaf names for mutual visibility
                let mut names = Vec::new();
                Self::extract_pattern_names(name_syn, &mut names);
                let mut leaf_bindings = HashMap::new();
                for (name, _name_scopes) in &names {
                    if *name != "_" {
                        let b = self.bind(name, &[], BindingScope::Local);
                        self.arena.get_mut(b).is_prebound = true;
                        // Immutability set later by analyze_destructure_pattern
                        leaf_bindings.insert(name.to_string(), b);
                    }
                }
                entries.push(LetrecEntry::Destructure {
                    pattern: name_syn,
                    value: value_syn,
                    leaf_bindings,
                });
            } else {
                return Err(format!(
                    "{}: letrec binding name must be a symbol or destructure pattern",
                    span
                ));
            }
            i += 2;
        }

        // Second pass: analyze values and build the output.
        // Simple bindings go into the Letrec node's bindings vec.
        // Destructured bindings: the temp binding AND all leaf bindings
        // go into the Letrec bindings vec (leaf bindings initialized to
        // nil). This ensures the lowerer allocates slots for all bindings
        // before lowering any lambda values — lambdas may capture
        // destructured leaf bindings. Destructure nodes in the body then
        // update the leaf binding slots.
        //
        // Seed signal_env for all pre-bound simple bindings with Silent.
        // Without this, forward-referenced letrec siblings default to
        // Signal::yields() (the unknown-binding fallback in
        // get_raw_callee_signal), causing spurious SuspendingCall
        // instructions. This matches analyze_file_letrec's optimistic
        // seeding strategy.
        for entry in &entries {
            if let LetrecEntry::Simple(binding, _) = entry {
                self.signal_env.insert(*binding, Signal::silent());
            }
        }

        let mut bindings = Vec::new();
        let mut destructures = Vec::new();
        let mut signal = Signal::silent();

        for entry in &entries {
            match entry {
                LetrecEntry::Simple(binding, value_syntax) => {
                    let value = self.analyze_expr(value_syntax)?;
                    signal = signal.combine(value.signal);
                    // Track signal and arity for interprocedural analysis
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_signals,
                        ..
                    } = &value.kind
                    {
                        self.signal_env.insert(*binding, *inferred_signals);
                        let arity = Arity::for_lambda(
                            rest_param.is_some(),
                            *num_required,
                            lambda_params.len(),
                        );
                        self.arity_env.insert(*binding, arity);
                    }
                    bindings.push((*binding, value));
                }
                LetrecEntry::Destructure {
                    pattern: pattern_syntax,
                    value: value_syntax,
                    leaf_bindings,
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    signal = signal.combine(value.signal);
                    // Create a temp binding for the value in the Letrec bindings
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    // Analyze the pattern using pre-created bindings from pass 1
                    // Immutable by default; individual leaves with @ opt into mutability
                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        true,
                        &span,
                    )?;
                    self.pre_bindings.clear();
                    // Add leaf bindings to the Letrec bindings vec (initialized
                    // to nil) so the lowerer allocates slots for them before
                    // lowering any lambda values that might capture them.
                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::silent(HirKind::Nil, span.clone())));
                    }
                    destructures.push((pattern, tmp));
                }
            }
        }

        let body = self.analyze_body(&items[2..], span.clone())?;
        signal = signal.combine(body.signal);

        self.pop_scope();

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::silent(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::silent(HirKind::Var(tmp), span.clone())),
                            strict: true,
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), signal)
        };

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(final_body),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_define(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        self.analyze_define_or_const(items, span, false)
    }

    pub(crate) fn analyze_const(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        self.analyze_define_or_const(items, span, true)
    }

    /// Shared implementation for `var` (mutable) and `def` (immutable) binding forms.
    fn analyze_define_or_const(
        &mut self,
        items: &[Syntax],
        span: Span,
        immutable: bool,
    ) -> Result<Hir, String> {
        let form = if immutable { "def" } else { "var" };

        if items.len() != 3 {
            return Err(format!("{}: {} requires name and value", span, form));
        }

        // Destructuring: (var (a b) expr) or (def [a b] expr)
        if Self::is_destructure_pattern(&items[1]) {
            let pattern =
                self.analyze_destructure_pattern(&items[1], BindingScope::Local, immutable, &span)?;
            let value = self.analyze_expr(&items[2])?;
            let signal = value.signal;
            return Ok(Hir::new(
                HirKind::Destructure {
                    pattern,
                    value: Box::new(value),
                    strict: true,
                },
                span,
                signal,
            ));
        }

        let raw_name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: {} name must be a symbol", span, form))?;
        let (name, at_mutable) = super::strip_at_prefix(raw_name);

        // Check if we're inside a function scope
        let in_function = self.scopes.iter().any(|s| s.is_function);

        // Check if the value is a lambda form
        let is_lambda_form = if let Some(list) = items[2].as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn")
        } else {
            false
        };

        if in_function {
            // Inside a function, creates a local binding
            let name_scopes = items[1].scopes.as_slice();
            let binding = if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                existing
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };

            if immutable && !at_mutable {
                self.arena.get_mut(binding).is_immutable = true;
            }

            // Seed signal_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.signal_env.insert(binding, Signal::silent());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
                        self.arity_env
                            .insert(binding, Self::arity_from_syntax_params(params_syn));
                    }
                }
            }

            // Now analyze the value (which can reference the binding)
            let value = self.analyze_expr(&items[2])?;

            // Update signal_env and arity_env with the actual inferred values
            if let HirKind::Lambda {
                params: lambda_params,
                num_required,
                rest_param,
                inferred_signals,
                ..
            } = &value.kind
            {
                self.signal_env.insert(binding, *inferred_signals);
                let arity =
                    Arity::for_lambda(rest_param.is_some(), *num_required, lambda_params.len());
                self.arity_env.insert(binding, arity);
            }

            let value_signal = value.signal;
            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                value_signal,
            ))
        } else {
            // At top level, creates a local binding.
            // Mark as prebound so that needs_capture() returns true when
            // the binding is captured by a lambda in the same begin block.
            // Without this, an immutable captured local would be captured
            // by value (nil) before its initializer runs.
            let name_scopes = items[1].scopes.as_slice();
            let binding = self.bind(name, name_scopes, BindingScope::Local);
            self.arena.get_mut(binding).is_prebound = true;

            if immutable && !at_mutable {
                self.arena.get_mut(binding).is_immutable = true;
            }

            // Seed signal_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.signal_env.insert(binding, Signal::silent());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
                        let arity = Self::arity_from_syntax_params(params_syn);
                        self.arity_env.insert(binding, arity);
                    }
                }
            }

            // Now analyze the value
            let value = self.analyze_expr(&items[2])?;

            // Update signal_env and arity_env with the actual inferred values
            if let HirKind::Lambda {
                params: lambda_params,
                num_required,
                rest_param,
                inferred_signals,
                ..
            } = &value.kind
            {
                self.signal_env.insert(binding, *inferred_signals);
                let arity =
                    Arity::for_lambda(rest_param.is_some(), *num_required, lambda_params.len());
                self.arity_env.insert(binding, arity);
            }

            let value_signal = value.signal;
            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                value_signal,
            ))
        }
    }

    pub(crate) fn analyze_assign(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: assign requires target and value", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: assign target must be a symbol", span))?;

        let target = match self.lookup(name, items[1].scopes.as_slice()) {
            Some(binding) => binding,
            None => {
                return Err(format!("{}: undefined variable: {}", span, name));
            }
        };

        // Check for immutable binding
        if self.arena.get(target).is_immutable {
            return Err(format!(
                "{}: cannot assign immutable binding '{}'",
                span, name
            ));
        }

        // Mark as mutated
        self.arena.get_mut(target).is_mutated = true;

        // Invalidate signal and arity tracking for this binding since it's being mutated
        // The binding's signal and arity are now uncertain
        self.signal_env.remove(&target);
        self.arity_env.remove(&target);

        let value = self.analyze_expr(&items[2])?;
        let signal = value.signal;

        Ok(Hir::new(
            HirKind::Assign {
                target,
                value: Box::new(value),
            },
            span,
            signal,
        ))
    }
}
