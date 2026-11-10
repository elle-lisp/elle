// audited: 2026-10-24
//! The binding forms `let`, `def`, `var` and `assign`, and the analysis state a
//! binding's initializer leaves for it.
//!
//! docs/bindings.md

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

        // Open this binding's scope, and analyze the value before the name is
        // bound, so the value sees the enclosing binding of the same name. The
        // rest of the chain is then analyzed inside the scope.
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
            self.record_lambda_facts(binding, &value);
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
            let destr = self.destructure(pattern, Hir::silent(HirKind::Var(tmp), span), true, span);
            signal = signal.combine(destr.signal);
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
            return Ok(self.destructure(pattern, value, true, span));
        }

        let raw_name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: {} name must be a symbol", span, form))?;
        let (name, at_mutable) = super::strip_at_prefix(raw_name);

        // Check if we're inside a function scope
        let in_function = self.scopes.iter().any(|s| s.is_function);

        if in_function {
            // Inside a function, reuse the binding `analyze_begin` pre-bound in
            // this scope, or bind a new local.
            let name_scopes = items[1].scopes.as_slice();
            let binding = if let Some(existing) = self.lookup_in_current_scope(name, name_scopes) {
                existing
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };

            if immutable && self.immutable_by_default && !at_mutable {
                self.arena.get_mut(binding).is_immutable = true;
            }

            let value = self.analyze_definition(binding, &items[2])?;
            // Initializer analyzed: later forms in the fn body may now
            // read this binding's value (letrec* left-to-right init).
            self.arena.get_mut(binding).init_pending = false;

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
            // At top level, bind a new local and mark it prebound. A sibling
            // lambda in the same `begin` may capture the binding before its
            // initializer runs, and `needs_capture` then gives it a cell where
            // capture by value would copy nil. A self-edge does not mark the
            // binding captured, so a binding only its own lambda names stays
            // cell-free.
            let name_scopes = items[1].scopes.as_slice();
            let binding = self.bind(name, name_scopes, BindingScope::Local);
            self.arena.get_mut(binding).is_prebound = true;

            if immutable && self.immutable_by_default && !at_mutable {
                self.arena.get_mut(binding).is_immutable = true;
            }

            let value = self.analyze_definition(binding, &items[2])?;

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

    /// Analyze the initializer of the `def` or `var` that binds `binding`, and
    /// record what it establishes.
    ///
    /// The value may refer to the binding. The self-recursion context of
    /// `analyze_initializer` classifies a self-edge directly inside its lambda
    /// `CaptureKind::Recursive`, which resolves to the executing closure, cell
    /// free, as in a self-recursive `letrec`. A self-reference reads the seed
    /// while the lambda is analyzed, and the solve raises it (docs/pipeline.md).
    fn analyze_definition(
        &mut self,
        binding: Binding,
        value_syntax: &Syntax,
    ) -> Result<Hir, String> {
        // A self-recursive call inside the lambda reads the seed, not the
        // unknown-callee fallback.
        self.seed_lambda_facts(binding, value_syntax);
        let value = self.analyze_initializer(binding, value_syntax)?;
        self.record_lambda_facts(binding, &value);
        self.apply_transient_binding_state(binding);
        Ok(value)
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

        // Mark as mutated, and as written by a closure when this `assign`
        // sits in a lambda the binding does not belong to. Assignment is
        // lexical, so no other route can write a binding from a closure.
        let from_closure = !self.is_binding_in_current_scope(target);
        let inner = self.arena.get_mut(target);
        inner.is_mutated = true;
        if from_closure {
            inner.mark_written_by_closure();
        }

        // Invalidate signal and arity tracking for this binding since it's being mutated
        // The binding's signal and arity are now uncertain
        self.signal_env.remove(&target);
        self.arity_env.remove(&target);
        self.bound_env.remove(&target);

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

    /// Consume the transient analysis state (the squelch signal) and apply it
    /// to a binding. Called after analyzing a binding's value expression in
    /// `def`, `let`, `letrec`, and file-scope letrec.
    pub(crate) fn apply_transient_binding_state(&mut self, binding: Binding) {
        // Compile-time squelch: the value was `(squelch spec f)` or
        // `(attune spec f)` with a literal spec
        if let Some(sig) = self.last_squelch_signal.take() {
            self.signal_env.insert(binding, sig);
        }
    }
}
