//! Lambda analysis: (fn (params...) body...)

use super::*;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::Value;

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_lambda(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: lambda requires parameters and body", span));
        }

        let params_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::Array(_)) {
                format!(
                    "{}: lambda parameters must use (...) or [...], not @[...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: lambda parameters must be a list (...) or [...], got {}",
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

        // Parse parameter list (handles &opt, &, &keys, &named)
        let parsed = Self::parse_params(params_syntax, &span)?;

        // Bind required parameters
        let mut params = Vec::new();
        let mut param_destructures = Vec::new();
        for param in parsed.required.iter() {
            if let Some(name) = param.as_symbol() {
                let binding = self.bind(name, param.scopes.as_slice(), BindingScope::Parameter);
                params.push(binding);
            } else if Self::is_destructure_pattern(param) {
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
        let num_required = params.len();

        // Bind optional parameters (same as required — they're all param slots)
        for param in parsed.optional.iter() {
            if let Some(name) = param.as_symbol() {
                let binding = self.bind(name, param.scopes.as_slice(), BindingScope::Parameter);
                params.push(binding);
            } else if Self::is_destructure_pattern(param) {
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

        // Handle collector (& / &keys / &named)
        use super::destructure::CollectorParams;
        use crate::hir::pattern::HirPattern;
        use crate::hir::VarargKind;
        let (rest_param, vararg_kind) = match parsed.collector {
            Some(CollectorParams::Rest(rest_syn)) => {
                let name = rest_syn
                    .as_symbol()
                    .ok_or_else(|| format!("{}: rest parameter after & must be a symbol", span))?;
                let binding = self.bind(name, rest_syn.scopes.as_slice(), BindingScope::Parameter);
                params.push(binding);
                (Some(binding), VarargKind::List)
            }
            Some(CollectorParams::Keys(keys_syn)) => {
                if let Some(name) = keys_syn.as_symbol() {
                    // &keys opts — simple symbol binding
                    let binding =
                        self.bind(name, keys_syn.scopes.as_slice(), BindingScope::Parameter);
                    params.push(binding);
                    (Some(binding), VarargKind::Struct)
                } else if Self::is_destructure_pattern(keys_syn) {
                    // &keys {:host h :port p} — destructure pattern
                    let tmp = self.bind("__keys_param", &[], BindingScope::Parameter);
                    params.push(tmp);
                    let pattern = self.analyze_destructure_pattern(
                        keys_syn,
                        BindingScope::Local,
                        false,
                        &span,
                    )?;
                    param_destructures.push((pattern, tmp));
                    (Some(tmp), VarargKind::Struct)
                } else {
                    return Err(format!(
                        "{}: &keys must be followed by a symbol or destructure pattern",
                        span
                    ));
                }
            }
            Some(CollectorParams::Named(named_syms)) => {
                // &named host port → synthetic binding + struct destructure
                let tmp = self.bind("__named_param", &[], BindingScope::Parameter);
                params.push(tmp);

                // Collect valid key names for strict validation
                let mut valid_keys = Vec::new();
                let mut entries = Vec::new();
                for sym_syntax in named_syms {
                    let name = sym_syntax.as_symbol().unwrap(); // validated by parse_params
                    valid_keys.push(name.to_string());

                    // Create a binding for each named param
                    let binding =
                        self.bind(name, sym_syntax.scopes.as_slice(), BindingScope::Local);
                    entries.push((name.to_string(), HirPattern::Var(binding)));
                }

                // Build struct destructure pattern: {:name1 name1 :name2 name2 ...}
                let pattern = HirPattern::Struct { entries };
                param_destructures.push((pattern, tmp));

                (Some(tmp), VarargKind::StrictStruct(valid_keys))
            }
            None => (None, VarargKind::List), // default; irrelevant when no rest_param
        };

        // Set current lambda params for effect source tracking
        self.current_lambda_params = params.clone();

        // Analyze body
        // Extract docstring if present (string literal as first of multiple body expressions)
        let body_items = &items[2..];
        let (doc, body_start) = if body_items.len() > 1 {
            if let SyntaxKind::String(s) = &body_items[0].kind {
                (Some(Value::string(s.clone())), &body_items[1..])
            } else {
                (None, body_items)
            }
        } else {
            (None, body_items)
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
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body: Box::new(body),
                num_locals,
                inferred_effect,
                doc,
            },
            span,
            Effect::none(),
        ))
    }
}
