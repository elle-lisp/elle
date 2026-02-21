//! Binding forms: let, let*, letrec, define, set!, lambda

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_let(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let requires bindings list", span));
        }

        let bindings_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: let bindings must be a list", span))?;

        // Phase 1: Analyze all value expressions in the OUTER scope
        let mut names_and_values = Vec::new();
        let mut effect = Effect::pure();

        for binding in bindings_syntax {
            let pair = binding
                .as_list()
                .ok_or_else(|| format!("{}: let binding must be a pair", span))?;
            if pair.len() != 2 {
                return Err(format!("{}: let binding must be (name value)", span));
            }

            let name = pair[0]
                .as_symbol()
                .ok_or_else(|| format!("{}: let binding name must be a symbol", span))?;
            let value = self.analyze_expr(&pair[1])?;
            effect = effect.combine(value.effect.clone());
            names_and_values.push((name, value));
        }

        // Phase 2: Push scope and create all bindings
        self.push_scope(false);

        let mut bindings = Vec::new();
        for (name, value) in names_and_values {
            let id = self.bind(
                name,
                BindingKind::Local {
                    index: self.current_local_index(),
                },
            );
            // Track effect for interprocedural analysis
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                self.effect_env.insert(id, inferred_effect.clone());
            }
            bindings.push((id, value));
        }

        // Analyze body expressions (empty body returns nil)
        let body = if items.len() > 2 {
            self.analyze_body(&items[2..], span.clone())?
        } else {
            Hir::pure(HirKind::Nil, span.clone())
        };
        effect = effect.combine(body.effect.clone());

        self.pop_scope();

        Ok(Hir::new(
            HirKind::Let {
                bindings,
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_let_star(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: let* requires bindings list", span));
        }

        let bindings_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: let* bindings must be a list", span))?;

        self.push_scope(false);

        let mut bindings = Vec::new();
        let mut effect = Effect::pure();

        for binding in bindings_syntax {
            let pair = binding
                .as_list()
                .ok_or_else(|| format!("{}: let* binding must be a pair", span))?;
            if pair.len() != 2 {
                return Err(format!("{}: let* binding must be (name value)", span));
            }

            let name = pair[0]
                .as_symbol()
                .ok_or_else(|| format!("{}: let* binding name must be a symbol", span))?;
            // In let*, each value CAN see previous bindings
            let value = self.analyze_expr(&pair[1])?;
            effect = effect.combine(value.effect.clone());

            let id = self.bind(
                name,
                BindingKind::Local {
                    index: self.current_local_index(),
                },
            );
            // Track effect for interprocedural analysis
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                self.effect_env.insert(id, inferred_effect.clone());
            }
            bindings.push((id, value));
        }

        let body = if items.len() > 2 {
            self.analyze_body(&items[2..], span.clone())?
        } else {
            Hir::pure(HirKind::Nil, span.clone())
        };
        effect = effect.combine(body.effect.clone());

        self.pop_scope();

        Ok(Hir::new(
            HirKind::Let {
                bindings,
                body: Box::new(body),
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
        let mut binding_ids = Vec::new();
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
            let id = self.bind(
                name,
                BindingKind::Local {
                    index: self.current_local_index(),
                },
            );
            binding_ids.push(id);
        }

        // Second pass: analyze values
        let mut bindings = Vec::new();
        let mut effect = Effect::pure();
        for (i, binding) in bindings_syntax.iter().enumerate() {
            let pair = binding.as_list().unwrap();
            let value = self.analyze_expr(&pair[1])?;
            effect = effect.combine(value.effect.clone());
            // Track effect for interprocedural analysis
            // Note: For mutual recursion, effects may be incomplete at this point
            // since later bindings haven't been analyzed yet. This is conservative.
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                self.effect_env
                    .insert(binding_ids[i], inferred_effect.clone());
            }
            bindings.push((binding_ids[i], value));
        }

        let body = self.analyze_body(&items[2..], span.clone())?;
        effect = effect.combine(body.effect.clone());

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

    /// Check if an expression is a define form and return the name being defined
    pub(crate) fn is_define_form(syntax: &Syntax) -> Option<&str> {
        if let SyntaxKind::List(items) = &syntax.kind {
            if let Some(first) = items.first() {
                if let Some(name) = first.as_symbol() {
                    if name == "define" {
                        if let Some(second) = items.get(1) {
                            return second.as_symbol();
                        }
                    }
                }
            }
        }
        None
    }

    pub(crate) fn analyze_define(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: define requires name and value", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: define name must be a symbol", span))?;
        let sym = self.symbols.intern(name);

        // Check if we're inside a function scope
        // If so, define creates a local binding, not a global one
        let in_function = self.scopes.iter().any(|s| s.is_function);

        // Check if the value is a lambda form (fn or lambda)
        // If so, we'll seed effect_env with Pure before analyzing so self-recursive
        // calls don't default to Yields
        let is_lambda_form = if let Some(list) = items[2].as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn" || s == "lambda")
        } else {
            false
        };

        if in_function {
            // Inside a function, define creates a local binding
            // Check if binding was pre-created by analyze_begin (for mutual recursion)
            let binding_id = if let Some(existing) = self.lookup_in_current_scope(name) {
                existing
            } else {
                // Not pre-created, create now (for single defines outside begin)
                let local_index = self.current_local_count();
                self.bind(name, BindingKind::Local { index: local_index })
            };

            // Seed effect_env with Pure for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding_id, Effect::pure());
            }

            // Now analyze the value (which can reference the binding)
            let value = self.analyze_expr(&items[2])?;

            // Update effect_env with the actual inferred effect
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                self.effect_env.insert(binding_id, inferred_effect.clone());
            }

            // Emit a LocalDefine that stores to a local slot
            Ok(Hir::new(
                HirKind::LocalDefine {
                    binding: binding_id,
                    value: Box::new(value),
                },
                span,
                Effect::pure(),
            ))
        } else {
            // At top level, define creates a global binding
            // Create binding first so recursive references work
            let binding_id = self.bind(name, BindingKind::Global);

            // Seed effect_env with Pure for lambda forms so self-recursive calls
            // don't default to Yields during analysis
            if is_lambda_form {
                self.effect_env.insert(binding_id, Effect::pure());
            }

            // Now analyze the value
            let value = self.analyze_expr(&items[2])?;

            // Update effect_env with the actual inferred effect
            // Also record in defined_global_effects for cross-form tracking
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                self.effect_env.insert(binding_id, inferred_effect.clone());
                // Record for cross-form effect tracking
                self.defined_global_effects
                    .insert(sym, inferred_effect.clone());
            }

            Ok(Hir::new(
                HirKind::Define {
                    name: sym,
                    value: Box::new(value),
                },
                span,
                Effect::pure(),
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

        let target = match self.lookup(name) {
            Some(id) => id,
            None => {
                // Treat as global reference (may have been defined in a previous form)
                let sym = self.symbols.intern(name);
                let id = self.ctx.fresh_binding();
                self.ctx.register_binding(BindingInfo::global(id, sym));
                id
            }
        };

        // Mark as mutated
        if let Some(info) = self.ctx.get_binding_mut(target) {
            info.mark_mutated();
        }

        // Invalidate effect tracking for this binding since it's being mutated
        // The binding's effect is now uncertain
        self.effect_env.remove(&target);

        let value = self.analyze_expr(&items[2])?;
        let effect = value.effect.clone();

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

        // Bind parameters
        let mut params = Vec::new();
        for (i, param) in params_syntax.iter().enumerate() {
            let name = param
                .as_symbol()
                .ok_or_else(|| format!("{}: lambda parameter must be a symbol", span))?;
            let id = self.bind(name, BindingKind::Parameter { index: i as u16 });
            params.push(id);
        }

        // Set current lambda params for effect source tracking
        self.current_lambda_params = params.clone();

        // Analyze body
        // Skip docstring if present (string literal as first body expression)
        let body_items = &items[2..];
        let body_start = if body_items.len() > 1 {
            // Check if first item is a string literal (docstring)
            if matches!(&body_items[0].kind, SyntaxKind::String(_)) {
                &body_items[1..] // Skip docstring
            } else {
                body_items
            }
        } else {
            body_items
        };
        let body = self.analyze_body(body_start, span.clone())?;
        let num_locals = self.current_local_count();

        // Compute the inferred effect based on effect sources
        let inferred_effect = self.compute_inferred_effect(&body, &params);

        self.pop_scope();
        let mut captures = std::mem::replace(&mut self.current_captures, saved_captures);
        self.parent_captures = saved_parent_captures;

        // Restore effect sources
        self.current_effect_sources = saved_effect_sources;
        self.current_lambda_params = saved_lambda_params;

        // Update is_mutated flag in captures based on current binding info
        // This is needed because the capture info might have been created before
        // the set! was analyzed, so is_mutated might be stale
        for cap in &mut captures {
            if let Some(info) = self.ctx.get_binding(cap.binding) {
                cap.is_mutated = info.is_mutated;
            }
        }

        // Propagate captures from this lambda to the parent lambda
        // If we're in a nested lambda, add our captures to the parent's captures
        // But only if:
        // 1. They're not parameters of the current lambda
        // 2. They're not already accessible in the parent scope (as params or locals)
        // 3. They're not already in the parent's captures
        for cap in &captures {
            // Check if this capture is a parameter of the current lambda
            let is_param = params.contains(&cap.binding);
            if is_param {
                continue;
            }

            // Check if already in parent's captures
            if self
                .current_captures
                .iter()
                .any(|c| c.binding == cap.binding)
            {
                continue;
            }

            // Check if the binding is accessible in the parent scope (without capturing)
            // This handles the case where the inner lambda captures a parameter of the outer lambda
            let is_in_parent_scope = self.is_binding_in_current_scope(cap.binding);
            if is_in_parent_scope {
                // The binding is accessible in the parent scope, no need to propagate
                continue;
            }

            // This capture is from an outer scope, propagate it to the parent lambda
            let propagated_cap = cap.clone();
            self.current_captures.push(propagated_cap);
        }

        // Lambda itself is pure, but captures the body's effect
        Ok(Hir::new(
            HirKind::Lambda {
                params,
                captures,
                body: Box::new(body),
                num_locals,
                inferred_effect,
            },
            span,
            Effect::pure(), // Creating a closure is pure
        ))
    }
}
