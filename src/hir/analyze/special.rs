//! Special forms: yield, match, silence

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

mod pattern;

impl<'a> Analyzer<'a> {
    /// Analyze a `(silence ...)` form.
    ///
    /// silence is a declaration, not an expression. It must appear inside
    /// a lambda body. It accumulates into `current_param_bounds` and
    /// `current_declared_ceiling`, which `analyze_lambda` reads after
    /// analyzing the body.
    ///
    /// Forms:
    /// - `(silence)` — function-level ceiling = silent
    /// - `(silence param)` — parameter bound = silent
    ///
    /// Signal keywords are not accepted. Use `(squelch ...)` instead.
    pub(crate) fn analyze_silence(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: silence must appear inside a function body",
                span
            ));
        }

        let args = &items[1..];
        if args.is_empty() {
            // (silence) — function-level ceiling = silent
            self.current_declared_ceiling = Some(Signal::silent());
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        match &args[0].kind {
            SyntaxKind::Keyword(_) => {
                // (silence :kw ...) — signal keywords are rejected here
                return Err(format!(
                    "{}: silence takes no signal keywords — use (squelch ...) instead",
                    span
                ));
            }
            SyntaxKind::Symbol(param_name) => {
                // (silence param) — parameter-level bound = silent
                let binding =
                    self.find_current_param_binding(param_name, &args[0].span, "silence")?;

                // No keywords allowed after the parameter name
                if !args[1..].is_empty() {
                    return Err(format!(
                        "{}: silence takes no signal keywords — use (squelch ...) instead",
                        span
                    ));
                }

                // Last wins for duplicate parameter bounds
                self.current_param_bounds.insert(binding, Signal::silent());
            }
            _ => {
                return Err(format!(
                    "{}: silence: expected parameter name, got {}",
                    args[0].span,
                    args[0].kind_label()
                ));
            }
        }

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
    /// attune! is a compile-time preamble declaration. It sets the function's
    /// signal ceiling to the specified bits — the function may emit at most
    /// these signals. Generalizes `(silence)`: `(silence)` is `(attune!)` with
    /// no signals permitted.
    ///
    /// Forms:
    /// - `(attune! :keyword)` — ceiling = single signal
    /// - `(attune! |:kw1 :kw2|)` — ceiling = set of signals
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

        let bits = self.resolve_static_signal(&args[0])?;
        self.current_declared_ceiling = Some(Signal {
            bits,
            propagates: 0,
        });

        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// Analyze a `(muffle signal-spec)` form.
    ///
    /// muffle is a declaration, not an expression. It must appear inside
    /// a lambda body. It absorbs specific signal bits from the body's
    /// inferred signal — they are allowed in the body but excluded from
    /// the function's external signal.
    ///
    /// When used with `(silence)`, muffled bits expand the ceiling:
    /// `(silence) (muffle :error)` allows `:error` in the body.
    /// Without `(silence)`, muffled bits are subtracted from the inferred signal.
    ///
    /// Forms:
    /// - `(muffle :keyword)` — absorb a single signal
    /// - `(muffle |:kw1 :kw2|)` — absorb a set of signals
    pub(crate) fn analyze_muffle(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: muffle must appear inside a function body",
                span
            ));
        }

        let args = &items[1..];
        if args.len() != 1 {
            return Err(format!(
                "{}: muffle requires exactly one argument (signal keyword or set)",
                span
            ));
        }

        let bits = self.resolve_static_signal(&args[0])?;
        self.current_muffle_bits |= bits;

        Ok(Hir::silent(HirKind::Nil, span))
    }

    /// `(emit <signal> <value>)` — general signal emission.
    ///
    /// The first argument must be a compile-time constant: a literal keyword
    /// (`:yield`, `:error`, `:io`, etc.) or a literal set of keywords
    /// (`|:yield :io|`). The analyzer extracts the signal bits at compile
    /// time and records them in the HIR node.
    pub(crate) fn analyze_emit(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 || items.len() > 3 {
            return Err(format!(
                "{}: emit requires 1 or 2 arguments (signal [value])",
                span
            ));
        }

        // Extract signal bits from the first argument (must be literal keyword or set)
        let signal_bits = self.resolve_static_signal(&items[1])?;

        let value = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            Hir::silent(HirKind::Nil, span.clone())
        };

        // Track direct signal emission — inherent to this function.
        self.current_signal_sources.direct_bits =
            self.current_signal_sources.direct_bits.union(signal_bits);

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
    ///
    /// Rejects non-literal arguments at compile time.
    fn resolve_static_signal(
        &self,
        syntax: &Syntax,
    ) -> Result<crate::value::fiber::SignalBits, String> {
        use crate::syntax::SyntaxKind;
        use crate::value::fiber::SignalBits;

        match &syntax.kind {
            SyntaxKind::Keyword(name) => {
                let registry = crate::signals::registry::global_registry().lock().unwrap();
                match registry.to_signal_bits(name) {
                    Some(bits) => Ok(bits),
                    None => Err(format!(
                        "{}: emit: unknown signal keyword :{}",
                        syntax.span, name
                    )),
                }
            }
            SyntaxKind::Set(elements) => {
                let registry = crate::signals::registry::global_registry().lock().unwrap();
                let mut bits = SignalBits::EMPTY;
                for elem in elements {
                    match &elem.kind {
                        SyntaxKind::Keyword(name) => match registry.to_signal_bits(name) {
                            Some(b) => bits |= b,
                            None => {
                                return Err(format!(
                                    "{}: emit: unknown signal keyword :{}",
                                    elem.span, name
                                ))
                            }
                        },
                        _ => {
                            return Err(format!(
                                "{}: emit: set elements must be keywords",
                                elem.span
                            ))
                        }
                    }
                }
                Ok(bits)
            }
            _ => Err(format!(
                "{}: emit: first argument must be a signal keyword or keyword set, got {:?}",
                syntax.span, syntax.kind
            )),
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
            arm_spans.push(args[i].span.clone());
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
            self.current_signal_sources.direct_bits = self
                .current_signal_sources
                .direct_bits
                .union(crate::value::SIG_ERROR);
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

    /// `(silent!)` — assert that the current function is silent (no signals).
    pub(crate) fn analyze_silence_assert(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: silent! must appear inside a function body",
                span
            ));
        }
        if items.len() != 1 {
            return Err(format!("{}: silent! takes no arguments", span));
        }
        self.current_silence_assert = true;
        Ok(Hir::silent(HirKind::Nil, span))
    }

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
