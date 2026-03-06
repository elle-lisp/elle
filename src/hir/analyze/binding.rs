//! Binding forms: let, letrec, define, set, file-letrec

use super::*;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_let(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let requires bindings list", span));
        }

        let bindings_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Array(_)) {
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
        enum LetBinding<'s> {
            Simple(&'s str, Vec<ScopeId>, Hir),
            Destructure(&'s Syntax, Hir),
        }
        let mut analyzed = Vec::new();
        let mut effect = Effect::inert();

        for binding in bindings_syntax {
            let pair = binding
                .as_list_or_tuple()
                .ok_or_else(|| format!("{}: let binding must be a pair (...) or [...]", span))?;
            if pair.len() != 2 {
                return Err(format!("{}: let binding must be (name value)", span));
            }

            let value = self.analyze_expr(&pair[1])?;
            effect = effect.combine(value.effect);

            if let Some(name) = pair[0].as_symbol() {
                analyzed.push(LetBinding::Simple(name, pair[0].scopes.clone(), value));
            } else if Self::is_destructure_pattern(&pair[0]) {
                analyzed.push(LetBinding::Destructure(&pair[0], value));
            } else {
                return Err(format!(
                    "{}: let binding name must be a symbol, list, or array",
                    span
                ));
            }
        }

        // Phase 2: Push scope and create all bindings
        self.push_scope(false);

        let mut bindings = Vec::new();
        let mut destructures = Vec::new();

        for item in analyzed {
            match item {
                LetBinding::Simple(name, name_scopes, value) => {
                    let binding = self.bind(name, &name_scopes, BindingScope::Local);
                    // Track effect and arity for interprocedural analysis
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_effect,
                        ..
                    } = &value.kind
                    {
                        self.effect_env.insert(binding, *inferred_effect);
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
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        false,
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
            Hir::inert(HirKind::Nil, span.clone())
        };
        effect = effect.combine(body.effect);

        self.pop_scope();

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::inert(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::inert(HirKind::Var(tmp), span.clone())),
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), effect)
        };

        Ok(Hir::new(
            HirKind::Let {
                bindings,
                body: Box::new(final_body),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_letrec(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: letrec requires bindings and body", span));
        }

        let bindings_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Array(_)) {
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
        enum LetrecEntry<'s> {
            Simple(Binding, &'s Syntax),
            Destructure(&'s Syntax, &'s Syntax),
        }
        let mut entries = Vec::new();

        for binding in bindings_syntax {
            let pair = binding
                .as_list_or_tuple()
                .ok_or_else(|| format!("{}: letrec binding must be a pair (...) or [...]", span))?;
            if pair.len() != 2 {
                return Err(format!("{}: letrec binding must be (name value)", span));
            }

            if let Some(name) = pair[0].as_symbol() {
                // Simple binding — bind immediately for mutual recursion.
                // Marked prebound: may be captured before initialization.
                let b = self.bind(name, pair[0].scopes.as_slice(), BindingScope::Local);
                b.mark_prebound();
                entries.push(LetrecEntry::Simple(b, &pair[1]));
            } else if Self::is_destructure_pattern(&pair[0]) {
                // Destructure pattern — pre-bind leaf names for mutual visibility
                let mut names = Vec::new();
                Self::extract_pattern_names(&pair[0], &mut names);
                for (name, name_scopes) in &names {
                    if *name != "_" {
                        let b = self.bind(name, name_scopes, BindingScope::Local);
                        b.mark_prebound();
                    }
                }
                entries.push(LetrecEntry::Destructure(&pair[0], &pair[1]));
            } else {
                return Err(format!(
                    "{}: letrec binding name must be a symbol or destructure pattern",
                    span
                ));
            }
        }

        // Second pass: analyze values and build the output.
        // Simple bindings go into the Letrec node's bindings vec.
        // Destructured bindings: the temp binding AND all leaf bindings
        // go into the Letrec bindings vec (leaf bindings initialized to
        // nil). This ensures the lowerer allocates slots for all bindings
        // before lowering any lambda values — lambdas may capture
        // destructured leaf bindings. Destructure nodes in the body then
        // update the leaf binding slots.
        let mut bindings = Vec::new();
        let mut destructures = Vec::new();
        let mut effect = Effect::inert();

        for entry in &entries {
            match entry {
                LetrecEntry::Simple(binding, value_syntax) => {
                    let value = self.analyze_expr(value_syntax)?;
                    effect = effect.combine(value.effect);
                    // Track effect and arity for interprocedural analysis
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_effect,
                        ..
                    } = &value.kind
                    {
                        self.effect_env.insert(*binding, *inferred_effect);
                        let arity = Arity::for_lambda(
                            rest_param.is_some(),
                            *num_required,
                            lambda_params.len(),
                        );
                        self.arity_env.insert(*binding, arity);
                    }
                    bindings.push((*binding, value));
                }
                LetrecEntry::Destructure(pattern_syntax, value_syntax) => {
                    let value = self.analyze_expr(value_syntax)?;
                    effect = effect.combine(value.effect);
                    // Create a temp binding for the value in the Letrec bindings
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    // Analyze the pattern (leaf bindings already exist from pass 1)
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        false,
                        &span,
                    )?;
                    // Add leaf bindings to the Letrec bindings vec (initialized
                    // to nil) so the lowerer allocates slots for them before
                    // lowering any lambda values that might capture them.
                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::inert(HirKind::Nil, span.clone())));
                    }
                    destructures.push((pattern, tmp));
                }
            }
        }

        let body = self.analyze_body(&items[2..], span.clone())?;
        effect = effect.combine(body.effect);

        self.pop_scope();

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::inert(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::inert(HirKind::Var(tmp), span.clone())),
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), effect)
        };

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(final_body),
            },
            span,
            effect,
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
            let in_function = self.scopes.iter().any(|s| s.is_function);
            let scope = if in_function {
                BindingScope::Local
            } else {
                BindingScope::Global
            };
            let pattern = self.analyze_destructure_pattern(&items[1], scope, immutable, &span)?;
            let value = self.analyze_expr(&items[2])?;
            let effect = value.effect;
            return Ok(Hir::new(
                HirKind::Destructure {
                    pattern,
                    value: Box::new(value),
                },
                span,
                effect,
            ));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: {} name must be a symbol", span, form))?;
        let sym = self.symbols.intern(name);

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

            if immutable {
                binding.mark_immutable();
            }

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::inert());
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

            // Update effect_env and arity_env with the actual inferred values
            if let HirKind::Lambda {
                params: lambda_params,
                num_required,
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                let arity =
                    Arity::for_lambda(rest_param.is_some(), *num_required, lambda_params.len());
                self.arity_env.insert(binding, arity);
            }

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::inert(),
            ))
        } else {
            // At top level, creates a global binding
            let binding = self.bind(name, &[], BindingScope::Global);
            self.user_defined_globals.insert(binding);

            if immutable {
                binding.mark_immutable();
                self.defined_immutable_globals.insert(sym);
            }

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::inert());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
                        let arity = Self::arity_from_syntax_params(params_syn);
                        self.arity_env.insert(binding, arity);
                        self.defined_global_arities.insert(sym, arity);
                    }
                }
            }

            // Now analyze the value
            let value = self.analyze_expr(&items[2])?;

            // Update effect_env and arity_env with the actual inferred values
            // Also record in defined_global_effects/arities for cross-form tracking
            if let HirKind::Lambda {
                params: lambda_params,
                num_required,
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                self.defined_global_effects.insert(sym, *inferred_effect);
                let arity =
                    Arity::for_lambda(rest_param.is_some(), *num_required, lambda_params.len());
                self.arity_env.insert(binding, arity);
                self.defined_global_arities.insert(sym, arity);
            }

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::inert(),
            ))
        }
    }

    /// Analyze a list of top-level forms as a synthetic letrec.
    ///
    /// Each form is classified as `Def` (immutable), `Var` (mutable), or
    /// `Expr` (gensym-named dummy binding). Two-pass analysis:
    /// - Pass 1: pre-bind all names (enables mutual recursion)
    /// - Pass 2: analyze initializers sequentially
    ///
    /// Returns a single `HirKind::Letrec` node. The body is a reference
    /// to the last binding (the file's return value).
    pub(crate) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        if forms.is_empty() {
            return Ok(Hir::pure(HirKind::Nil, span));
        }

        self.push_scope(false);

        // Classify each form and collect binding info for pass 1.
        enum PreBound<'s> {
            Simple {
                binding: Binding,
                value_syntax: &'s Syntax,
            },
            Destructure {
                pattern_syntax: &'s Syntax,
                value_syntax: &'s Syntax,
                immutable: bool,
            },
        }

        let mut entries: Vec<PreBound> = Vec::new();
        let mut gensym_counter = 0u32;

        // Pass 1: pre-bind all names for mutual visibility.
        for form in &forms {
            match form {
                FileForm::Def(name_syntax, value_syntax) => {
                    if let Some(name) = name_syntax.as_symbol() {
                        let binding =
                            self.bind(name, name_syntax.scopes.as_slice(), BindingScope::Local);
                        binding.mark_prebound();
                        binding.mark_immutable();

                        // Seed effect_env and arity_env for lambda forms
                        if Self::is_lambda_syntax(value_syntax) {
                            self.effect_env.insert(binding, Effect::none());
                            if let Some(list) = value_syntax.as_list() {
                                if let Some(params_syn) =
                                    list.get(1).and_then(|s| s.as_list_or_tuple())
                                {
                                    self.arity_env.insert(
                                        binding,
                                        Self::arity_from_syntax_params(params_syn),
                                    );
                                }
                            }
                        }

                        entries.push(PreBound::Simple {
                            binding,
                            value_syntax,
                        });
                    } else if Self::is_destructure_pattern(name_syntax) {
                        // Pre-bind leaf names for mutual visibility
                        let mut names = Vec::new();
                        Self::extract_pattern_names(name_syntax, &mut names);
                        for (name, name_scopes) in &names {
                            if *name != "_" {
                                let b = self.bind(name, name_scopes, BindingScope::Local);
                                b.mark_prebound();
                                b.mark_immutable();
                            }
                        }
                        entries.push(PreBound::Destructure {
                            pattern_syntax: name_syntax,
                            value_syntax,
                            immutable: true,
                        });
                    } else {
                        return Err(format!(
                            "{}: def name must be a symbol or destructure pattern",
                            name_syntax.span
                        ));
                    }
                }
                FileForm::Var(name_syntax, value_syntax) => {
                    if let Some(name) = name_syntax.as_symbol() {
                        let binding =
                            self.bind(name, name_syntax.scopes.as_slice(), BindingScope::Local);
                        binding.mark_prebound();
                        // var is mutable — do NOT mark_immutable

                        // Seed effect_env and arity_env for lambda forms
                        if Self::is_lambda_syntax(value_syntax) {
                            self.effect_env.insert(binding, Effect::none());
                            if let Some(list) = value_syntax.as_list() {
                                if let Some(params_syn) =
                                    list.get(1).and_then(|s| s.as_list_or_tuple())
                                {
                                    self.arity_env.insert(
                                        binding,
                                        Self::arity_from_syntax_params(params_syn),
                                    );
                                }
                            }
                        }

                        entries.push(PreBound::Simple {
                            binding,
                            value_syntax,
                        });
                    } else if Self::is_destructure_pattern(name_syntax) {
                        let mut names = Vec::new();
                        Self::extract_pattern_names(name_syntax, &mut names);
                        for (name, name_scopes) in &names {
                            if *name != "_" {
                                let b = self.bind(name, name_scopes, BindingScope::Local);
                                b.mark_prebound();
                                // var — mutable
                            }
                        }
                        entries.push(PreBound::Destructure {
                            pattern_syntax: name_syntax,
                            value_syntax,
                            immutable: false,
                        });
                    } else {
                        return Err(format!(
                            "{}: var name must be a symbol or destructure pattern",
                            name_syntax.span
                        ));
                    }
                }
                FileForm::Expr(expr_syntax) => {
                    let gensym_name = format!("__file_expr_{}", gensym_counter);
                    gensym_counter += 1;
                    let binding = self.bind(&gensym_name, &[], BindingScope::Local);
                    binding.mark_prebound();
                    entries.push(PreBound::Simple {
                        binding,
                        value_syntax: expr_syntax,
                    });
                }
            }
        }

        // Pass 2: analyze all initializers sequentially.
        let mut bindings = Vec::new();
        let mut effect = Effect::none();
        let mut last_binding: Option<Binding> = None;

        for entry in &entries {
            match entry {
                PreBound::Simple {
                    binding,
                    value_syntax,
                    ..
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    effect = effect.combine(value.effect);

                    // Update effect_env and arity_env with actual inferred values
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_effect,
                        ..
                    } = &value.kind
                    {
                        self.effect_env.insert(*binding, *inferred_effect);
                        let arity = Arity::for_lambda(
                            rest_param.is_some(),
                            *num_required,
                            lambda_params.len(),
                        );
                        self.arity_env.insert(*binding, arity);
                    }

                    bindings.push((*binding, value));
                    last_binding = Some(*binding);
                }
                PreBound::Destructure {
                    pattern_syntax,
                    value_syntax,
                    immutable,
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    effect = effect.combine(value.effect);

                    // Create a temp binding for the value in the Letrec bindings
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));

                    // Analyze the pattern (leaf bindings already exist from pass 1)
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        *immutable,
                        &span,
                    )?;

                    // Add leaf bindings to the Letrec bindings vec (initialized
                    // to nil) so the lowerer allocates slots for them.
                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::pure(HirKind::Nil, span.clone())));
                        last_binding = Some(*leaf_binding);
                    }

                    // Emit the destructure inline as a gensym binding whose
                    // initializer is the Destructure node. This ensures the
                    // destructure runs in sequence with other initializers,
                    // not deferred to the body (which would be too late for
                    // subsequent initializers that reference the leaf bindings).
                    let destructure_hir = Hir::pure(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::pure(HirKind::Var(tmp), span.clone())),
                        },
                        span.clone(),
                    );
                    let destr_gensym = format!("__file_destr_{}", gensym_counter);
                    gensym_counter += 1;
                    let destr_binding = self.bind(&destr_gensym, &[], BindingScope::Local);
                    bindings.push((destr_binding, destructure_hir));
                }
            }
        }

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::pure(HirKind::Var(binding), span.clone()),
            None => Hir::pure(HirKind::Nil, span.clone()),
        };

        self.pop_scope();

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    /// Check if a syntax node is a lambda form: `(fn ...)`.
    fn is_lambda_syntax(syntax: &Syntax) -> bool {
        if let Some(list) = syntax.as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn")
        } else {
            false
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
                // Treat as global reference (may have been defined in a previous form)
                let sym = self.symbols.intern(name);
                // Check if this was declared const in a previous form
                if self.immutable_globals.contains(&sym) {
                    return Err(format!(
                        "{}: cannot assign immutable binding '{}'",
                        span, name
                    ));
                }
                Binding::new(sym, BindingScope::Global)
            }
        };

        // Check for immutable binding
        if target.is_immutable() {
            return Err(format!(
                "{}: cannot assign immutable binding '{}'",
                span, name
            ));
        }

        // Mark as mutated
        target.mark_mutated();

        // Invalidate effect and arity tracking for this binding since it's being mutated
        // The binding's effect and arity are now uncertain
        self.effect_env.remove(&target);
        self.arity_env.remove(&target);

        let value = self.analyze_expr(&items[2])?;
        let effect = value.effect;

        Ok(Hir::new(
            HirKind::Assign {
                target,
                value: Box::new(value),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_set(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: assign requires target and value", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: assign target must be a symbol", span))?;

        let target = match self.lookup(name, items[1].scopes.as_slice()) {
            Some(binding) => binding,
            None => {
                // Treat as global reference (may have been defined in a previous form)
                let sym = self.symbols.intern(name);
                // Check if this was declared const in a previous form
                if self.immutable_globals.contains(&sym) {
                    return Err(format!(
                        "{}: cannot assign immutable binding '{}'",
                        span, name
                    ));
                }
                Binding::new(sym, BindingScope::Global)
            }
        };

        // Check for immutable binding
        if target.is_immutable() {
            return Err(format!(
                "{}: cannot assign immutable binding '{}'",
                span, name
            ));
        }

        // Mark as mutated
        target.mark_mutated();

        // Invalidate effect and arity tracking for this binding since it's being mutated
        // The binding's effect and arity are now uncertain
        self.effect_env.remove(&target);
        self.arity_env.remove(&target);

        let value = self.analyze_expr(&items[2])?;
        let effect = value.effect;

        Ok(Hir::new(
            HirKind::Assign {
                target,
                value: Box::new(value),
            },
            span,
            effect,
        ))
    }
}
