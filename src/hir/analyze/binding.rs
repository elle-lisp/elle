//! Binding forms: let, letrec, define, set

use super::*;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_let(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let requires bindings list", span));
        }

        let bindings_syntax = items[1].as_list().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Tuple(_) | SyntaxKind::Array(_)) {
                format!(
                    "{}: let bindings must use parentheses ((name value) ...), \
                     not brackets [...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: let bindings must be a parenthesized list ((name value) ...), \
                     got {}",
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

        let bindings_syntax = items[1].as_list().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Tuple(_) | SyntaxKind::Array(_)) {
                format!(
                    "{}: letrec bindings must use parentheses ((name value) ...), \
                     not brackets [...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: letrec bindings must be a parenthesized list ((name value) ...), \
                     got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;

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

            Ok(Hir::new(
                HirKind::Define {
                    binding,
                    value: Box::new(value),
                },
                span,
                Effect::none(),
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
            return Err(format!("{}: set requires target and value", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: set target must be a symbol", span))?;

        let target = match self.lookup(name, items[1].scopes.as_slice()) {
            Some(binding) => binding,
            None => {
                // Treat as global reference (may have been defined in a previous form)
                let sym = self.symbols.intern(name);
                // Check if this was declared const in a previous form
                if self.immutable_globals.contains(&sym) {
                    return Err(format!("{}: cannot set immutable binding '{}'", span, name));
                }
                Binding::new(sym, BindingScope::Global)
            }
        };

        // Check for immutable binding
        if target.is_immutable() {
            return Err(format!("{}: cannot set immutable binding '{}'", span, name));
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
}
