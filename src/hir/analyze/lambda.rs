//! Lambda analysis: (fn (params...) body...)

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_lambda(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: lambda requires parameters and body", span));
        }

        let params_syntax = items[1].as_list().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Tuple(_) | SyntaxKind::Array(_)) {
                format!(
                    "{}: lambda parameters must use parentheses (params...), \
                     not brackets [...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: lambda parameters must be a parenthesized list (params...), \
                     got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;

        // Save current captures and parent captures, start fresh for this lambda
        let saved_captures = std::mem::take(&mut self.current_captures);
        let saved_parent_captures = std::mem::take(&mut self.parent_captures);

        // Save and reset effect sources for polymorphic inference
        let saved_effect_sources = std::mem::take(&mut self.current_effect_sources);
        let saved_lambda_params = std::mem::take(&mut self.current_lambda_params);

        // For nested lambdas, the parent captures are the captures from the enclosing lambda
        self.parent_captures = saved_captures.clone();

        // Increment fn_depth so break cannot target blocks outside this lambda
        self.fn_depth += 1;

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
        self.fn_depth -= 1;
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
