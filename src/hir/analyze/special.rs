// audited: 2026-10-11
//! Special forms: the signal declarations, the assertions, emit, and match.
//!
//! docs/signals/inference.md

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

mod pattern;

impl<'a> Analyzer<'a> {
    /// Analyze a `(bound param spec)` form.
    ///
    /// bound is a declaration, not an expression. It must appear inside a
    /// lambda body, and it records into `current_param_bounds`, which
    /// `analyze_lambda` reads after analyzing the body. `spec` is a signal
    /// keyword or a literal set of them, `||` included.
    pub(crate) fn analyze_bound(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: bound must appear inside a function body",
                span
            ));
        }
        if items.len() != 3 {
            return Err(format!(
                "{}: bound requires a parameter name and a signal spec",
                span
            ));
        }
        let Some(param_name) = items[1].as_symbol() else {
            return Err(format!(
                "{}: bound: expected parameter name, got {}",
                items[1].span,
                items[1].kind_label()
            ));
        };
        let binding = self.find_current_param_binding(param_name, &items[1].span, "bound")?;
        if self.current_param_bounds.contains_key(&binding) {
            return Err(format!(
                "{}: bound: parameter {} is already bounded",
                span, param_name
            ));
        }
        let bits = self.resolve_static_signal("bound", &items[2])?;
        self.current_param_bounds.insert(binding, Signal::of(bits));
        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// Find a parameter binding by name in the current lambda's params.
    fn find_current_param_binding(
        &self,
        name: &str,
        span: &Span,
        form_name: &str,
    ) -> Result<Binding, String> {
        for param in &self.current_lambda_params {
            if self.symbols.name(self.arena.get(*param).name) == Some(name) {
                return Ok(*param);
            }
        }
        Err(format!(
            "{}: {}: '{}' is not a parameter of this function",
            span, form_name, name
        ))
    }

    /// Analyze an `(attune! signal-spec)` form.
    ///
    /// attune! is a compile-time declaration. It records a ceiling that
    /// `analyze_lambda` checks the inferred signal against; the function's
    /// signal stays the inferred one. A body that declares several ceilings
    /// must fit each, so they intersect.
    ///
    /// Forms:
    /// - `(attune! :keyword)` — ceiling = single signal
    /// - `(attune! |:kw1 :kw2|)` — ceiling = set of signals, `||` included
    pub(crate) fn analyze_attune_assert(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: attune! must appear inside a function body",
                span
            ));
        }

        let args = &items[1..];
        if args.len() != 1 {
            return Err(format!(
                "{}: attune! requires exactly one argument (signal keyword or set)",
                span
            ));
        }

        let bits = self.resolve_static_signal("attune!", &args[0])?;
        self.declare_ceiling(bits);
        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// `(silent!)` — the empty ceiling, `(attune! ||)` by another name.
    pub(crate) fn analyze_silent(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: silent! must appear inside a function body",
                span
            ));
        }
        if items.len() != 1 {
            return Err(format!("{}: silent! takes no arguments", span));
        }
        self.declare_ceiling(crate::value::fiber::SignalBits::EMPTY);
        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// Narrow the current lambda's ceiling to `bits`.
    fn declare_ceiling(&mut self, bits: crate::value::fiber::SignalBits) {
        let bits = match self.current_declared_ceiling {
            Some(ceiling) => ceiling.bits.intersection(bits),
            None => bits,
        };
        self.current_declared_ceiling = Some(Signal::of(bits));
    }

    /// `(emit <signal> <value>)` — general signal emission.
    ///
    /// The first argument must be a compile-time constant: a literal keyword
    /// (`:yield`, `:error`, `:io`, etc.), a literal set of keywords
    /// (`|:yield :io|`), or that set quoted (`is_static_emit_spec`). The
    /// analyzer extracts the signal bits at compile time and records them in
    /// the HIR node.
    pub(crate) fn analyze_emit(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 || items.len() > 3 {
            return Err(format!(
                "{}: emit requires 1 or 2 arguments (signal [value])",
                span
            ));
        }

        // Extract signal bits from the first argument (must be literal keyword or set)
        let signal_bits = self.resolve_static_signal("emit", &items[1])?;

        let value = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            Hir::silent(HirKind::Nil, span)
        };

        // Track direct signal emission — inherent to this function.
        self.add_inherent_bits(signal_bits);

        let signal = Signal {
            bits: signal_bits,
            propagates: 0,
        };

        Ok(Hir::new(
            HirKind::Emit {
                signal: signal_bits,
                value: Box::new(value),
            },
            span,
            signal,
        ))
    }

    /// Resolve a static signal specifier (keyword or set literal) to SignalBits.
    ///
    /// Accepts:
    /// - A literal keyword: `:yield` → SIG_YIELD
    /// - A literal set of keywords: `|:yield :io|` → SIG_YIELD | SIG_IO
    /// - The same set as `(set :yield :io)`, the shape a quoted set literal
    ///   takes, so a form built as data and handed to `eval` reads the same
    ///
    /// Rejects non-literal arguments at compile time. `form` is the form the
    /// spec belongs to, and every message names it.
    fn resolve_static_signal(
        &self,
        form: &str,
        syntax: &Syntax,
    ) -> Result<crate::value::fiber::SignalBits, String> {
        use crate::syntax::SyntaxKind;
        use crate::value::fiber::SignalBits;

        let resolve = |name: &str, span: &Span| {
            crate::signals::registry::with_registry(|r| r.to_signal_bits(name))
                .ok_or_else(|| format!("{}: {}: unknown signal keyword :{}", span, form, name))
        };
        let union = |elements: &[Syntax]| {
            let mut bits = SignalBits::EMPTY;
            for elem in elements {
                let SyntaxKind::Keyword(name) = &elem.kind else {
                    return Err(format!(
                        "{}: {}: set elements must be keywords",
                        elem.span, form
                    ));
                };
                bits |= resolve(name, &elem.span)?;
            }
            Ok(bits)
        };
        match &syntax.kind {
            SyntaxKind::Keyword(name) => resolve(name, &syntax.span),
            SyntaxKind::Set(elements) => union(elements),
            SyntaxKind::List(items) if items.first().and_then(Syntax::as_symbol) == Some("set") => {
                union(&items[1..])
            }
            _ => Err(format!(
                "{}: {}: expected a signal keyword or keyword set, got {}",
                syntax.span,
                form,
                syntax.kind_label()
            )),
        }
    }

    /// Whether `emit` can resolve `syntax` at compile time: a keyword, a set
    /// literal, or the list a quoted set literal becomes. The list must call
    /// the `set` primitive with keywords alone. A binding that shadows `set`
    /// builds whatever it builds, and an element the list computes is known
    /// only at run time, so either leaves the emit dynamic
    /// (docs/signals/emit.md).
    pub(crate) fn is_static_emit_spec(&mut self, syntax: &Syntax) -> bool {
        match &syntax.kind {
            SyntaxKind::Keyword(_) | SyntaxKind::Set(_) => true,
            SyntaxKind::List(items) => {
                let Some((head, elements)) = items.split_first() else {
                    return false;
                };
                if head.as_symbol() != Some("set")
                    || !elements
                        .iter()
                        .all(|e| matches!(e.kind, SyntaxKind::Keyword(_)))
                {
                    return false;
                }
                // Resolve `set` as the call would (forms/expr.rs, the symbol arm).
                let binding = self
                    .lookup("set", head.scopes.as_slice())
                    .or_else(|| self.lookup("set", &[]));
                binding.is_some_and(|b| {
                    let inner = self.arena.get(b);
                    inner.is_primitive && inner.name == crate::value::SymbolId::of("set")
                })
            }
            _ => false,
        }
    }

    pub(crate) fn analyze_match(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 4 {
            return Err(format!("{}: match requires value, pattern, and body", span));
        }

        let value = self.analyze_expr(&items[1])?;
        let mut signal = value.signal;
        let mut arms = Vec::new();
        let mut arm_spans = Vec::new();

        // Flat parsing: (match val pat1 body1 pat2 when guard body2 ...)
        let args = &items[2..];
        let mut i = 0;
        while i < args.len() {
            if i + 1 >= args.len() {
                return Err(format!(
                    "{}: match arm at position {} has pattern but no body",
                    span, i
                ));
            }

            self.push_scope(false);
            arm_spans.push(args[i].span);
            let pattern = self.analyze_pattern(&args[i])?;

            // Check for guard: pattern when guard body
            let (guard, body_idx) = if i + 3 < args.len() && args[i + 1].as_symbol() == Some("when")
            {
                let guard_expr = self.analyze_expr(&args[i + 2])?;
                signal = signal.combine(guard_expr.signal);
                (Some(guard_expr), i + 3)
            } else {
                (None, i + 1)
            };

            let body = self.analyze_expr(&args[body_idx])?;
            signal = signal.combine(body.signal);
            self.pop_scope();

            arms.push((pattern, guard, body));
            i = body_idx + 1;
        }

        // A match with no guardless irrefutable arm can fail at runtime
        // (:match-error) — signal inference must see the error capability,
        // both on the node and as an inherent source (like a direct emit).
        let total = arms
            .iter()
            .any(|(p, g, _)| g.is_none() && p.is_irrefutable());
        if !total {
            signal = signal.combine(Signal::errors());
            self.add_inherent_bits(crate::value::SIG_ERROR);
        }

        // Reachability check: an arm no value can reach is a compile-time error
        let dead = crate::hir::decision::unreachable_arms(&arms);
        if let Some(&idx) = dead.first() {
            return Err(format!(
                "{}: unreachable match arm {}: earlier arms already match every value this pattern matches",
                arm_spans[idx],
                idx + 1
            ));
        }

        // Redundancy check: an or-pattern alternative that earlier arms or
        // alternatives already cover is a compile-time error
        if let Some(dead) = crate::hir::decision::first_dead_alternative(&arms) {
            return Err(format!(
                "{}: unreachable or-pattern alternative {} in match arm {}: earlier arms or alternatives already match every value it matches",
                arm_spans[dead.arm],
                dead.alternative + 1,
                dead.arm + 1
            ));
        }

        Ok(Hir::new(
            HirKind::Match {
                value: Box::new(value),
                arms,
            },
            span,
            signal,
        ))
    }

    // === Compile-time assertion forms (! suffix) ===

    /// `(numeric!)` — assert that the current function is GPU-eligible
    /// (all parameters are numeric, enabling type check elision).
    pub(crate) fn analyze_numeric_assert(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: numeric! must appear inside a function body",
                span
            ));
        }
        if items.len() != 1 {
            return Err(format!("{}: numeric! takes no arguments", span));
        }
        self.current_numeric_assert = true;
        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// `(unicode! major [minor [patch]])` — declare the Unicode generation
    /// this source assumes. The generation is locked per program before VM
    /// construction, so the declaration is checked against the lock and
    /// evaluates to nil; a mismatch or a non-vendored request is a compile
    /// error. `(unicode!)` with no arguments is the query form: it folds to
    /// the same HIR the literal `[major minor patch]` produces. The LSP and
    /// lint analyzers check against the process default, so a file meant
    /// for `--unicode=16` shows a conflict diagnostic there.
    pub(crate) fn analyze_unicode(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        let locked = self.unicode_generation;
        if items.len() == 1 {
            let (major, minor, patch) = locked.version();
            // Fold to the same HIR the literal `[major minor patch]` produces,
            // by analyzing its elements rather than synthesizing an `Array`
            // node: the analyzer holds no syntax arena, and it needs none —
            // integers carry no region payload.
            let elems: Vec<Syntax> = [major, minor, patch]
                .iter()
                .map(|c| Syntax::new(SyntaxKind::Int(*c as i64), span))
                .collect();
            return self.analyze_collection_literal(
                "array",
                &elems,
                crate::hir::analyze::forms::expr::SpliceRule::Spread,
                span,
            );
        }
        if items.len() > 4 {
            return Err(format!(
                "{}: unicode! takes at most 3 version components",
                span
            ));
        }
        let mut request = Vec::new();
        for item in &items[1..] {
            match item.kind {
                SyntaxKind::Int(n) if n >= 0 => request.push(n),
                _ => {
                    return Err(format!(
                        "{}: unicode! components must be non-negative integer literals",
                        item.span
                    ))
                }
            }
        }
        let declared = crate::segment::Generation::from_request(&request)
            .map_err(|e| format!("{}: {}", span, e))?;
        if declared != locked {
            return Err(format!(
                "{}: unicode! declares Unicode {}, but this program runs under Unicode {}",
                span,
                declared.version_string(),
                locked.version_string()
            ));
        }
        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// `(immutable! x)` — assert that binding `x` is never assigned in the body.
    pub(crate) fn analyze_immutable_assert(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: immutable! must appear inside a function body",
                span
            ));
        }
        if items.len() != 2 {
            return Err(format!(
                "{}: immutable! requires exactly one argument (a symbol)",
                span
            ));
        }
        let name = items[1].as_symbol().ok_or_else(|| {
            format!(
                "{}: immutable! argument must be a symbol, got {}",
                items[1].span,
                items[1].kind_label()
            )
        })?;
        let binding = self
            .lookup(name, items[1].scopes.as_slice())
            .ok_or_else(|| format!("{}: immutable!: undefined variable '{}'", span, name))?;
        self.current_immutability_asserts.insert(binding);
        Ok(Hir::silent(HirKind::Nil, span))
    }
}
