//! Binding forms: let, let*, letrec, define, set!, lambda

use super::*;
use crate::hir::pattern::HirPattern;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_let(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let requires bindings list", span));
        }

        let bindings_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: let bindings must be a list", span))?;

        // Phase 1: Analyze all value expressions in the OUTER scope.
        // For destructuring bindings, we record the pattern syntax for Phase 2.
        enum LetBinding<'s> {
            Simple(&'s str, Vec<ScopeId>, Hir),
            Destructure(&'s Syntax, Hir),
        }
        let mut analyzed = Vec::new();
        let mut effect = Effect::none();

        for binding in bindings_syntax {
            let pair = binding
                .as_list()
                .ok_or_else(|| format!("{}: let binding must be a pair", span))?;
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
                        rest_param,
                        inferred_effect,
                        ..
                    } = &value.kind
                    {
                        self.effect_env.insert(binding, *inferred_effect);
                        let arity = if rest_param.is_some() {
                            Arity::AtLeast(lambda_params.len() - 1)
                        } else {
                            Arity::Exact(lambda_params.len())
                        };
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
            Hir::pure(HirKind::Nil, span.clone())
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
                    Hir::pure(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::pure(HirKind::Var(tmp), span.clone())),
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

        let bindings_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: letrec bindings must be a list", span))?;

        self.push_scope(false);

        // First pass: bind all names (for mutual recursion)
        let mut binding_handles = Vec::new();
        for binding in bindings_syntax {
            let pair = binding
                .as_list()
                .ok_or_else(|| format!("{}: letrec binding must be a pair", span))?;
            if pair.len() != 2 {
                return Err(format!("{}: letrec binding must be (name value)", span));
            }
            let name = pair[0]
                .as_symbol()
                .ok_or_else(|| format!("{}: letrec binding name must be a symbol", span))?;
            let b = self.bind(name, pair[0].scopes.as_slice(), BindingScope::Local);
            binding_handles.push(b);
        }

        // Second pass: analyze values
        let mut bindings = Vec::new();
        let mut effect = Effect::none();
        for (i, binding) in bindings_syntax.iter().enumerate() {
            let pair = binding.as_list().unwrap();
            let value = self.analyze_expr(&pair[1])?;
            effect = effect.combine(value.effect);
            // Track effect and arity for interprocedural analysis
            // Note: For mutual recursion, effects may be incomplete at this point
            // since later bindings haven't been analyzed yet. This is conservative.
            if let HirKind::Lambda {
                params: lambda_params,
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding_handles[i], *inferred_effect);
                let arity = if rest_param.is_some() {
                    Arity::AtLeast(lambda_params.len() - 1)
                } else {
                    Arity::Exact(lambda_params.len())
                };
                self.arity_env.insert(binding_handles[i], arity);
            }
            bindings.push((binding_handles[i], value));
        }

        let body = self.analyze_body(&items[2..], span.clone())?;
        effect = effect.combine(body.effect);

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

    /// Check if an expression is a var or def form and return all names being defined.
    /// For simple defines like `(def x ...)`, returns one name.
    /// For destructuring like `(def (a b) ...)`, returns all leaf names.
    pub(crate) fn is_define_form(syntax: &Syntax) -> Vec<(&str, &[ScopeId])> {
        if let SyntaxKind::List(items) = &syntax.kind {
            if let Some(first) = items.first() {
                if let Some(name) = first.as_symbol() {
                    if name == "var" || name == "def" {
                        if let Some(second) = items.get(1) {
                            // Simple symbol binding
                            if let Some(sym) = second.as_symbol() {
                                return vec![(sym, second.scopes.as_slice())];
                            }
                            // Destructuring pattern — extract all leaf symbols
                            let mut names = Vec::new();
                            Self::extract_pattern_names(second, &mut names);
                            return names;
                        }
                    }
                }
            }
        }
        Vec::new()
    }

    /// Recursively extract all symbol names from a syntax pattern (list or array).
    fn extract_pattern_names<'s>(syntax: &'s Syntax, out: &mut Vec<(&'s str, &'s [ScopeId])>) {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" || name == "&" => {
                // Skip wildcard and rest marker
            }
            SyntaxKind::Symbol(name) => {
                out.push((name.as_str(), syntax.scopes.as_slice()));
            }
            SyntaxKind::List(items) | SyntaxKind::Array(items) => {
                for item in items {
                    Self::extract_pattern_names(item, out);
                }
            }
            _ => {} // Ignore non-symbol, non-compound elements
        }
    }

    /// Check if a syntax node is a destructuring pattern (list or array of symbols/patterns).
    fn is_destructure_pattern(syntax: &Syntax) -> bool {
        matches!(&syntax.kind, SyntaxKind::List(_) | SyntaxKind::Array(_))
    }

    /// Estimate arity from syntax-level parameter list (before analysis).
    /// Detects `&` for variadic functions.
    fn arity_from_syntax_params(params: &[Syntax]) -> Arity {
        let has_rest = params
            .iter()
            .any(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
        if has_rest {
            // Count fixed params (everything before &)
            let fixed = params
                .iter()
                .position(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"))
                .unwrap_or(params.len());
            Arity::AtLeast(fixed)
        } else {
            Arity::Exact(params.len())
        }
    }

    /// Split a pattern's items at `&` into (fixed_elements, optional_rest).
    /// Validates: at most one `&`, exactly one pattern after `&`, `&` not at end.
    pub(super) fn split_rest_pattern<'s>(
        items: &'s [Syntax],
        span: &Span,
    ) -> Result<(&'s [Syntax], Option<&'s Syntax>), String> {
        let amp_pos = items
            .iter()
            .position(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
        match amp_pos {
            None => Ok((items, None)),
            Some(pos) => {
                // Check for multiple &
                let second = items[pos + 1..]
                    .iter()
                    .any(|s| matches!(&s.kind, SyntaxKind::Symbol(n) if n == "&"));
                if second {
                    return Err(format!("{}: multiple & in pattern", span));
                }
                let remaining = &items[pos + 1..];
                if remaining.len() != 1 {
                    return Err(format!(
                        "{}: & must be followed by exactly one pattern",
                        span
                    ));
                }
                Ok((&items[..pos], Some(&remaining[0])))
            }
        }
    }

    /// Convert a syntax pattern into an HirPattern, creating bindings for each leaf symbol.
    /// `scope` determines whether bindings are Local or Global.
    /// `immutable` determines whether bindings are marked immutable (def vs var).
    fn analyze_destructure_pattern(
        &mut self,
        syntax: &Syntax,
        scope: BindingScope,
        immutable: bool,
        span: &Span,
    ) -> Result<HirPattern, String> {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" => Ok(HirPattern::Wildcard),
            SyntaxKind::Symbol(name) => {
                let in_function = self.scopes.iter().any(|s| s.is_function);
                let binding_scope = if in_function {
                    BindingScope::Local
                } else {
                    scope
                };

                let binding = if in_function {
                    // Check if pre-created by analyze_begin
                    let name_scopes = syntax.scopes.as_slice();
                    if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                        existing
                    } else {
                        self.bind(name, name_scopes, binding_scope)
                    }
                } else if matches!(binding_scope, BindingScope::Global) {
                    self.bind(name, &[], binding_scope)
                } else {
                    self.bind(name, syntax.scopes.as_slice(), binding_scope)
                };

                if immutable {
                    binding.mark_immutable();
                }
                Ok(HirPattern::Var(binding))
            }
            SyntaxKind::List(items) => {
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, span)?;
                let mut elements = Vec::new();
                for item in fixed {
                    elements.push(self.analyze_destructure_pattern(item, scope, immutable, span)?);
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(
                        self.analyze_destructure_pattern(r, scope, immutable, span)?,
                    )),
                    None => None,
                };
                Ok(HirPattern::List { elements, rest })
            }
            SyntaxKind::Array(items) => {
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, span)?;
                let mut elements = Vec::new();
                for item in fixed {
                    elements.push(self.analyze_destructure_pattern(item, scope, immutable, span)?);
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(
                        self.analyze_destructure_pattern(r, scope, immutable, span)?,
                    )),
                    None => None,
                };
                Ok(HirPattern::Array { elements, rest })
            }
            _ => Err(format!(
                "{}: destructuring pattern element must be a symbol, list, or array",
                span
            )),
        }
    }

    pub(crate) fn analyze_define(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: var requires name and value", span));
        }

        // Destructuring: (var (a b) expr) or (var [a b] expr)
        if Self::is_destructure_pattern(&items[1]) {
            let in_function = self.scopes.iter().any(|s| s.is_function);
            let scope = if in_function {
                BindingScope::Local
            } else {
                BindingScope::Global
            };
            let pattern = self.analyze_destructure_pattern(&items[1], scope, false, &span)?;
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
            .ok_or_else(|| format!("{}: var name must be a symbol", span))?;

        // Check if we're inside a function scope
        // If so, var creates a local binding, not a global one
        let in_function = self.scopes.iter().any(|s| s.is_function);

        // Check if the value is a lambda form (fn or lambda)
        let is_lambda_form = if let Some(list) = items[2].as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn")
        } else {
            false
        };

        if in_function {
            // Inside a function, var creates a local binding
            // Check if binding was pre-created by analyze_begin (for mutual recursion)
            let name_scopes = items[1].scopes.as_slice();
            let binding = if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                existing
            } else {
                // Not pre-created, create now (for single vars outside begin)
                self.bind(name, name_scopes, BindingScope::Local)
            };

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::none());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list()) {
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
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                let arity = if rest_param.is_some() {
                    Arity::AtLeast(lambda_params.len() - 1)
                } else {
                    Arity::Exact(lambda_params.len())
                };
                self.arity_env.insert(binding, arity);
            }

            // Emit a Define (the lowerer checks binding.is_global())
            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::none(),
            ))
        } else {
            // At top level, var creates a global binding
            let sym = self.symbols.intern(name);
            let binding = self.bind(name, &[], BindingScope::Global);
            self.user_defined_globals.insert(binding);

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::none());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list()) {
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
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                self.defined_global_effects.insert(sym, *inferred_effect);
                let arity = if rest_param.is_some() {
                    Arity::AtLeast(lambda_params.len() - 1)
                } else {
                    Arity::Exact(lambda_params.len())
                };
                self.arity_env.insert(binding, arity);
                self.defined_global_arities.insert(sym, arity);
            }

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::none(),
            ))
        }
    }

    pub(crate) fn analyze_const(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: def requires name and value", span));
        }

        // Destructuring: (def (a b) expr) or (def [a b] expr)
        if Self::is_destructure_pattern(&items[1]) {
            let in_function = self.scopes.iter().any(|s| s.is_function);
            let scope = if in_function {
                BindingScope::Local
            } else {
                BindingScope::Global
            };
            let pattern = self.analyze_destructure_pattern(&items[1], scope, true, &span)?;
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
            .ok_or_else(|| format!("{}: def name must be a symbol", span))?;
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
            // Inside a function, def creates a local binding
            let name_scopes = items[1].scopes.as_slice();
            let binding = if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                existing
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };

            // Mark as immutable
            binding.mark_immutable();

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::none());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list()) {
                        self.arity_env
                            .insert(binding, Self::arity_from_syntax_params(params_syn));
                    }
                }
            }

            // Now analyze the value
            let value = self.analyze_expr(&items[2])?;

            // Update effect_env and arity_env with the actual inferred values
            if let HirKind::Lambda {
                params: lambda_params,
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                let arity = if rest_param.is_some() {
                    Arity::AtLeast(lambda_params.len() - 1)
                } else {
                    Arity::Exact(lambda_params.len())
                };
                self.arity_env.insert(binding, arity);
            }

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::none(),
            ))
        } else {
            // At top level, def creates a global binding
            let binding = self.bind(name, &[], BindingScope::Global);
            self.user_defined_globals.insert(binding);

            // Mark as immutable
            binding.mark_immutable();

            // Record in defined_immutable_globals for cross-form tracking
            self.defined_immutable_globals.insert(sym);

            // Seed effect_env and arity_env for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding, Effect::none());
                // Pre-seed arity from syntax (count params in the lambda form)
                if let Some(list) = items[2].as_list() {
                    if let Some(params_syn) = list.get(1).and_then(|s| s.as_list()) {
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
                rest_param,
                inferred_effect,
                ..
            } = &value.kind
            {
                self.effect_env.insert(binding, *inferred_effect);
                self.defined_global_effects.insert(sym, *inferred_effect);
                let arity = if rest_param.is_some() {
                    Arity::AtLeast(lambda_params.len() - 1)
                } else {
                    Arity::Exact(lambda_params.len())
                };
                self.arity_env.insert(binding, arity);
                self.defined_global_arities.insert(sym, arity);
            }

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::none(),
            ))
        }
    }

    pub(crate) fn analyze_set(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: set! requires target and value", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: set! target must be a symbol", span))?;

        let target = match self.lookup(name, items[1].scopes.as_slice()) {
            Some(binding) => binding,
            None => {
                // Treat as global reference (may have been defined in a previous form)
                let sym = self.symbols.intern(name);
                // Check if this was declared const in a previous form
                if self.immutable_globals.contains(&sym) {
                    return Err(format!(
                        "{}: cannot set! immutable binding '{}'",
                        span, name
                    ));
                }
                Binding::new(sym, BindingScope::Global)
            }
        };

        // Check for immutable binding
        if target.is_immutable() {
            return Err(format!(
                "{}: cannot set! immutable binding '{}'",
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
            HirKind::Set {
                target,
                value: Box::new(value),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_lambda(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: lambda requires parameters and body", span));
        }

        let params_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: lambda parameters must be a list", span))?;

        // Save current captures and parent captures, start fresh for this lambda
        let saved_captures = std::mem::take(&mut self.current_captures);
        let saved_parent_captures = std::mem::take(&mut self.parent_captures);

        // Save and reset effect sources for polymorphic inference
        let saved_effect_sources = std::mem::take(&mut self.current_effect_sources);
        let saved_lambda_params = std::mem::take(&mut self.current_lambda_params);

        // For nested lambdas, the parent captures are the captures from the enclosing lambda
        self.parent_captures = saved_captures.clone();

        self.push_scope(true);

        // Split params at & for variadic rest parameter
        let (fixed_params, rest_syntax) = Self::split_rest_pattern(params_syntax, &span)?;

        // Bind fixed parameters — some may be destructuring patterns
        let mut params = Vec::new();
        let mut param_destructures = Vec::new();
        for param in fixed_params.iter() {
            if let Some(name) = param.as_symbol() {
                let binding = self.bind(name, param.scopes.as_slice(), BindingScope::Parameter);
                params.push(binding);
            } else if Self::is_destructure_pattern(param) {
                // Create a tmp parameter binding; destructure in body
                let tmp = self.bind("__destructure_param", &[], BindingScope::Parameter);
                params.push(tmp);
                let pattern =
                    self.analyze_destructure_pattern(param, BindingScope::Local, false, &span)?;
                param_destructures.push((pattern, tmp));
            } else {
                return Err(format!(
                    "{}: lambda parameter must be a symbol, list, or array",
                    span
                ));
            }
        }

        // Bind rest parameter if present — it occupies a parameter slot
        // that the VM will fill with a list of extra arguments
        let rest_param = if let Some(rest_syn) = rest_syntax {
            let name = rest_syn
                .as_symbol()
                .ok_or_else(|| format!("{}: rest parameter after & must be a symbol", span))?;
            let binding = self.bind(name, rest_syn.scopes.as_slice(), BindingScope::Parameter);
            params.push(binding);
            Some(binding)
        } else {
            None
        };

        // Set current lambda params for effect source tracking
        self.current_lambda_params = params.clone();

        // Analyze body
        // Skip docstring if present (string literal as first body expression)
        let body_items = &items[2..];
        let body_start = if body_items.len() > 1 {
            if matches!(&body_items[0].kind, SyntaxKind::String(_)) {
                &body_items[1..]
            } else {
                body_items
            }
        } else {
            body_items
        };
        let body = self.analyze_body(body_start, span.clone())?;

        // If there are destructured parameters, wrap the body
        let body = if param_destructures.is_empty() {
            body
        } else {
            let body_effect = body.effect;
            let mut exprs: Vec<Hir> = param_destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::pure(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::pure(HirKind::Var(tmp), span.clone())),
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), body_effect)
        };

        let num_locals = self.current_local_count();

        // Compute the inferred effect based on effect sources
        let inferred_effect = self.compute_inferred_effect(&body, &params);

        self.pop_scope();
        let captures = std::mem::replace(&mut self.current_captures, saved_captures);
        self.parent_captures = saved_parent_captures;

        // Restore effect sources
        self.current_effect_sources = saved_effect_sources;
        self.current_lambda_params = saved_lambda_params;

        // No need to sync is_mutated — CaptureInfo reads from the shared Binding directly

        // Propagate captures from this lambda to the parent lambda
        for cap in &captures {
            let is_param = params.contains(&cap.binding);
            if is_param {
                continue;
            }

            if self
                .current_captures
                .iter()
                .any(|c| c.binding == cap.binding)
            {
                continue;
            }

            let is_in_parent_scope = self.is_binding_in_current_scope(cap.binding);
            if is_in_parent_scope {
                continue;
            }

            let propagated_cap = cap.clone();
            self.current_captures.push(propagated_cap);
        }

        // Lambda itself is pure, but captures the body's effect
        Ok(Hir::new(
            HirKind::Lambda {
                params,
                rest_param,
                captures,
                body: Box::new(body),
                num_locals,
                inferred_effect,
            },
            span,
            Effect::none(),
        ))
    }
}
