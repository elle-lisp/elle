//! Lambda analysis: (fn (params...) body...)

use super::*;
use crate::hir::expr::ParamBound;
use crate::signals::registry;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::Value;
use std::rc::Rc;

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_lambda(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: lambda requires parameters and body", span));
        }

        let params_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::ArrayMut(_)) {
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

        // Save and reset signal sources for polymorphic inference
        let saved_signal_sources = std::mem::take(&mut self.current_signal_sources);
        let saved_lambda_params = std::mem::take(&mut self.current_lambda_params);

        // Save and reset restrict accumulators
        let saved_param_bounds = std::mem::take(&mut self.current_param_bounds);
        let saved_declared_ceiling = self.current_declared_ceiling.take();
        let saved_muffle_bits = std::mem::replace(
            &mut self.current_muffle_bits,
            crate::value::fiber::SignalBits::EMPTY,
        );
        // Save and reset assertion accumulators
        let saved_silence_assert = std::mem::replace(&mut self.current_silence_assert, false);
        let saved_numeric_assert = std::mem::replace(&mut self.current_numeric_assert, false);
        let saved_immutability_asserts = std::mem::take(&mut self.current_immutability_asserts);

        // For nested lambdas, the parent captures are the captures from the enclosing lambda
        self.parent_captures = saved_captures.clone();

        // Increment fn_depth so break cannot target blocks outside this lambda
        self.fn_depth += 1;

        self.push_scope(true);

        // Parse parameter list (handles &opt, &, &keys, &named)
        let parsed = Self::parse_params(params_syntax, &span)?;

        // Bind required parameters
        let mut params = Vec::new();
        // Each entry: (pattern, binding, strict)
        // strict=true: missing/wrong-type values signal error (required and &keys patterns)
        // strict=false: missing/wrong-type values produce nil (&opt patterns, &named)
        let mut param_destructures: Vec<(_, _, bool)> = Vec::new();
        for param in parsed.required.iter() {
            if let Some(raw_name) = param.as_symbol() {
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let binding = self.bind(name, param.scopes.as_slice(), BindingScope::Parameter);
                if self.immutable_by_default && !is_mutable {
                    self.arena.get_mut(binding).is_immutable = true;
                }
                params.push(binding);
            } else if Self::is_destructure_pattern(param) {
                let tmp = self.bind("__destructure_param", &[], BindingScope::Parameter);
                params.push(tmp);
                // Immutable by default; individual leaves with @ opt into mutability
                let pattern = self.analyze_destructure_pattern(
                    param,
                    BindingScope::Local,
                    self.immutable_by_default,
                    &span,
                )?;
                // Required params: strict — wrong type should error
                param_destructures.push((pattern, tmp, true));
            } else {
                return Err(format!(
                    "{}: lambda parameter must be a symbol, list, or array",
                    span
                ));
            }
        }
        let num_required = params.len();

        // Bind optional parameters: strict=false because absent opt params receive nil
        for param in parsed.optional.iter() {
            if let Some(raw_name) = param.as_symbol() {
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let binding = self.bind(name, param.scopes.as_slice(), BindingScope::Parameter);
                if self.immutable_by_default && !is_mutable {
                    self.arena.get_mut(binding).is_immutable = true;
                }
                params.push(binding);
            } else if Self::is_destructure_pattern(param) {
                let tmp = self.bind("__destructure_param", &[], BindingScope::Parameter);
                params.push(tmp);
                // Immutable by default; individual leaves with @ opt into mutability
                let pattern = self.analyze_destructure_pattern(
                    param,
                    BindingScope::Local,
                    self.immutable_by_default,
                    &span,
                )?;
                // Optional params: strict=false — absent (nil) produces nil, not error
                param_destructures.push((pattern, tmp, false));
            } else {
                return Err(format!(
                    "{}: lambda parameter must be a symbol, list, or array",
                    span
                ));
            }
        }

        // Handle collector (& / &keys / &named)
        use super::destructure::CollectorParams;
        use crate::hir::pattern::{HirPattern, PatternKey};
        use crate::hir::VarargKind;
        let (rest_param, vararg_kind) = match parsed.collector {
            Some(CollectorParams::Rest(rest_syn)) => {
                let raw_name = rest_syn
                    .as_symbol()
                    .ok_or_else(|| format!("{}: rest parameter after & must be a symbol", span))?;
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let binding = self.bind(name, rest_syn.scopes.as_slice(), BindingScope::Parameter);
                if self.immutable_by_default && !is_mutable {
                    self.arena.get_mut(binding).is_immutable = true;
                }
                params.push(binding);
                (Some(binding), VarargKind::List)
            }
            Some(CollectorParams::Keys(keys_syn)) => {
                if let Some(raw_name) = keys_syn.as_symbol() {
                    // &keys opts — simple symbol binding
                    let (name, is_mutable) = super::strip_at_prefix(raw_name);
                    let binding =
                        self.bind(name, keys_syn.scopes.as_slice(), BindingScope::Parameter);
                    if self.immutable_by_default && !is_mutable {
                        self.arena.get_mut(binding).is_immutable = true;
                    }
                    params.push(binding);
                    (Some(binding), VarargKind::Struct)
                } else if Self::is_destructure_pattern(keys_syn) {
                    // &keys {:host h :port p} — destructure pattern
                    let tmp = self.bind("__keys_param", &[], BindingScope::Parameter);
                    params.push(tmp);
                    let pattern = self.analyze_destructure_pattern(
                        keys_syn,
                        BindingScope::Local,
                        self.immutable_by_default,
                        &span,
                    )?;
                    // &keys {:k v} destructures strictly: missing keys signal an error.
                    // Use &named or &keys opts (simple symbol) for optional kwargs.
                    param_destructures.push((pattern, tmp, true));
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
                    let raw_name = sym_syntax.as_symbol().unwrap(); // validated by parse_params
                    let (name, is_mutable) = super::strip_at_prefix(raw_name);
                    valid_keys.push(name.to_string());

                    // Create a binding for each named param
                    let binding =
                        self.bind(name, sym_syntax.scopes.as_slice(), BindingScope::Local);
                    if self.immutable_by_default && !is_mutable {
                        self.arena.get_mut(binding).is_immutable = true;
                    }
                    entries.push((
                        PatternKey::Keyword(name.to_string()),
                        HirPattern::Var(binding),
                    ));
                }

                // Build named-param destructure pattern: {:name1 name1 :name2 name2 ...}
                // Uses NamedStruct (not Struct) so missing keys produce nil, not errors.
                let pattern = HirPattern::NamedStruct { entries };
                // NamedStruct always uses StructGetOrNil; strict=false is consistent but unused.
                param_destructures.push((pattern, tmp, false));

                (Some(tmp), VarargKind::StrictStruct(valid_keys))
            }
            None => (None, VarargKind::List), // default; irrelevant when no rest_param
        };

        // Set current lambda params for signal source tracking
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

        // Analyze body — restrict forms within will populate
        // current_param_bounds and current_declared_ceiling
        let body = self.analyze_body(body_start, span.clone())?;

        // If there are destructured parameters, wrap the body
        let body = if param_destructures.is_empty() {
            body
        } else {
            let body_signal = body.signal;
            let mut exprs: Vec<Hir> = param_destructures
                .into_iter()
                .map(|(pattern, tmp, strict)| {
                    Hir::silent(
                        HirKind::Destructure {
                            pattern,
                            strict,
                            value: Box::new(Hir::silent(HirKind::Var(tmp), span.clone())),
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), body_signal)
        };

        let num_locals = self.current_local_count();

        // Compute the inferred signal based on signal sources.
        // Must happen before draining current_param_bounds, since
        // compute_inferred_signal reads them for bounded params.
        let mut inferred_signals = self.compute_inferred_signal(&body, &params);

        // Check assert-silent assertion (before ceiling/muffle adjustments)
        if self.current_silence_assert && (inferred_signals != Signal::silent()) {
            let reg = registry::global_registry().lock().unwrap();
            return Err(format!(
                "{}: assert-silent assertion failed: function may emit {}",
                span,
                reg.format_signal_bits(inferred_signals.bits),
            ));
        }

        // Check assert-immutable assertions
        for binding in &self.current_immutability_asserts {
            if self.arena.get(*binding).is_mutated {
                let name = self
                    .symbols
                    .name(self.arena.get(*binding).name)
                    .unwrap_or("?");
                return Err(format!(
                    "{}: assert-immutable assertion failed: '{}' is assigned in body",
                    span, name
                ));
            }
        }

        // Read numeric! assertion flag (will be placed on HIR Lambda)
        let assert_numeric = self.current_numeric_assert;

        // Read bound accumulators (populated by analyze_silence during body analysis)
        let param_bounds: Vec<ParamBound> = self
            .current_param_bounds
            .drain()
            .map(|(binding, signal)| ParamBound { binding, signal })
            .collect();
        let declared_ceiling = self.current_declared_ceiling.take();
        let muffle_bits = std::mem::replace(
            &mut self.current_muffle_bits,
            crate::value::fiber::SignalBits::EMPTY,
        );

        // When (silence) is declared, verify the body's inferred signal
        // fits within the ceiling.  Muffled bits expand the ceiling —
        // they are allowed in the body but excluded from the external signal.
        if let Some(ceiling) = &declared_ceiling {
            let effective_ceiling = ceiling.bits | muffle_bits;
            let excess = inferred_signals.bits.subtract(effective_ceiling);
            if !excess.is_empty() {
                let reg = registry::global_registry().lock().unwrap();
                return Err(format!(
                    "{}: function restricted to {} but body may emit {}",
                    span,
                    reg.format_signal_bits(ceiling.bits),
                    reg.format_signal_bits(excess),
                ));
            }
            if ceiling.propagates == 0 && inferred_signals.propagates != 0 {
                return Err(format!(
                    "{}: function restricted to silent but body is polymorphic",
                    span,
                ));
            }
            inferred_signals = *ceiling;
        } else if !muffle_bits.is_empty() {
            // No silence, but muffle is active: subtract muffled bits
            // from the inferred signal.
            inferred_signals.bits = inferred_signals.bits.subtract(muffle_bits);
        }

        self.pop_scope();
        self.fn_depth -= 1;
        let captures = std::mem::replace(&mut self.current_captures, saved_captures);
        self.parent_captures = saved_parent_captures;

        // Restore signal sources, restrict accumulators, and assertion accumulators
        self.current_signal_sources = saved_signal_sources;
        self.current_lambda_params = saved_lambda_params;
        self.current_param_bounds = saved_param_bounds;
        self.current_declared_ceiling = saved_declared_ceiling;
        self.current_muffle_bits = saved_muffle_bits;
        self.current_silence_assert = saved_silence_assert;
        self.current_numeric_assert = saved_numeric_assert;
        self.current_immutability_asserts = saved_immutability_asserts;

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

        // Capture the original lambda syntax for eval environment reconstruction
        let original_syntax = Some(Rc::new(Syntax::new(
            SyntaxKind::List(items.to_vec()),
            span.clone(),
        )));

        // Lambda itself is pure, but captures the body's signal
        Ok(Hir::new(
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body: Box::new(body),
                num_locals,
                inferred_signals,
                param_bounds,
                doc,
                syntax: original_syntax,
                assert_numeric,
            },
            span,
            Signal::silent(),
        ))
    }
}
