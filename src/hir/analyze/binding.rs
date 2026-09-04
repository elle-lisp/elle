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

        // Sequential bindings (Clojure-style): each binding sees all
        // previous bindings. Implemented by nesting single-binding lets.
        // (let [a 1 b (+ a 1)] body) → (let [a 1] (let [b (+ a 1)] body))
        if bindings_syntax.len() % 2 != 0 {
            return Err(format!(
                "{}: let bindings must have an even number of forms (name/value pairs)",
                span
            ));
        }

        self.analyze_sequential_let(bindings_syntax, &items[2..], span)
    }

    /// Recursively build nested single-binding lets for sequential semantics.
    /// Each binding pair becomes its own Let scope so the next pair sees it.
    fn analyze_sequential_let(
        &mut self,
        bindings: &[Syntax],
        body_items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if bindings.is_empty() {
            // Base case: no more bindings — analyze body
            return if body_items.is_empty() {
                Ok(Hir::silent(HirKind::Nil, span))
            } else {
                self.analyze_body(body_items, span)
            };
        }

        let name_syn = &bindings[0];
        let value_syn = &bindings[1];
        let rest = &bindings[2..];

        // Analyze this single binding using the original (non-sequential)
        // let machinery: push scope, analyze value in outer scope, create
        // binding, then analyze inner body (which is the rest of the chain).
        self.push_scope(false);

        let value = self.analyze_expr(value_syn)?;
        let mut signal = value.signal;

        let mut let_bindings = Vec::new();
        let mut destructure = None;

        if let Some(name) = name_syn.as_symbol() {
            let (actual_name, is_mutable) = super::strip_at_prefix(name);
            let binding = self.bind(actual_name, &name_syn.scopes, BindingScope::Local);
            if self.immutable_by_default && !is_mutable {
                self.arena.get_mut(binding).is_immutable = true;
            }
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
            self.apply_transient_binding_state(binding);
            let_bindings.push((binding, value));
        } else if Self::is_destructure_pattern(name_syn) {
            let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
            let_bindings.push((tmp, value));
            let pattern = self.analyze_destructure_pattern(
                name_syn,
                BindingScope::Local,
                self.immutable_by_default,
                &span,
            )?;
            destructure = Some((pattern, tmp));
        } else {
            return Err(format!(
                "{}: let binding name must be a symbol, list, or array",
                span
            ));
        }

        // Recursively analyze remaining bindings + body as the inner expression
        let inner = self.analyze_sequential_let(rest, body_items, span)?;
        signal = signal.combine(inner.signal);

        self.pop_scope();

        // Wrap with destructure if needed
        let final_body = if let Some((pattern, tmp)) = destructure {
            let destr = Hir::silent(
                HirKind::Destructure {
                    pattern,
                    value: Box::new(Hir::silent(HirKind::Var(tmp), span)),
                    strict: true,
                },
                span,
            );
            Hir::new(HirKind::Begin(vec![destr, inner]), span, signal)
        } else {
            inner
        };

        Ok(Hir::new(
            HirKind::Let {
                bindings: let_bindings,
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
            // Initializer analyzed: the destructured leaves (prebound by
            // analyze_begin Pass 1 in a fn body) are initialized once the
            // pattern runs.
            for leaf in &pattern.bindings().bindings {
                self.arena.get_mut(*leaf).init_pending = false;
            }
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

            if immutable && self.immutable_by_default && !at_mutable {
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

            // Now analyze the value (which can reference the binding). The
            // self-recursion context lets a self-edge inside the lambda classify
            // `CaptureKind::Recursive` (a recursive `def` nested in a lambda resolves its
            // self-edge to the executing closure — cell-free — exactly like a
            // self-recursive `letrec`).
            let value = self.analyze_initializer(binding, &items[2])?;
            // Initializer analyzed: later forms in the fn body may now
            // read this binding's value (letrec* left-to-right init).
            self.arena.get_mut(binding).init_pending = false;

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
            self.apply_transient_binding_state(binding);

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
            // Mark as prebound so that needs_capture() returns true when a SIBLING
            // lambda in the same begin block captures the binding (mutual recursion /
            // a forward reference) — such a captured immutable local would otherwise be
            // captured by value (nil) before its initializer runs. `is_prebound` only
            // forces a cell in combination with `is_captured` (`hir/arena.rs`
            // `needs_capture`), and a self-edge does not mark the binding captured, so a
            // *purely* self-recursive top-level binding stays cell-free despite being
            // prebound; its self-reference resolves to the executing closure.
            let name_scopes = items[1].scopes.as_slice();
            let binding = self.bind(name, name_scopes, BindingScope::Local);
            self.arena.get_mut(binding).is_prebound = true;

            if immutable && self.immutable_by_default && !at_mutable {
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

            // Now analyze the value, with the self-recursion context set so a
            // self-edge inside the lambda classifies `CaptureKind::Recursive`.
            let value = self.analyze_initializer(binding, &items[2])?;

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
            self.apply_transient_binding_state(binding);

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

        // Reject assignment to immutable bindings
        if self.arena.get(target).is_immutable {
            let binding_name = self
                .symbols
                .name(self.arena.get(target).name)
                .unwrap_or("?");
            return Err(format!(
                "{}: cannot assign immutable binding '{}' (use @{} to make it mutable)",
                span, binding_name, binding_name
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

    /// Consume transient analysis state (import projection, squelch signal)
    /// and apply them to a binding. Called after analyzing a binding's value
    /// expression in `def`, `let`, `letrec`, and file-scope letrec.
    pub(crate) fn apply_transient_binding_state(&mut self, binding: Binding) {
        // Import projection: the value was `((import "literal"))`
        if let Some(proj) = self.last_import_projection.take() {
            self.projection_env.insert(binding, proj);
        }
        // Compile-time squelch: the value was `(squelch f mask)`
        if let Some(sig) = self.last_squelch_signal.take() {
            self.signal_env.insert(binding, sig);
        }
    }
}
