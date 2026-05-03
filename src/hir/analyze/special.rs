//! Special forms: yield, match, silence

use super::*;
use crate::hir::pattern::{HirPattern, PatternKey, PatternLiteral};
use crate::syntax::{Syntax, SyntaxKind};

/// Callback type for resolving variable patterns.
/// In normal mode, creates new bindings. In or-pattern reuse mode, looks up existing bindings.
type ResolveVar<'a> =
    dyn Fn(&mut Analyzer<'_>, &str, &[ScopeId], &Span) -> Result<HirPattern, String> + 'a;

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
                // (silence :kw ...) — keywords are no longer accepted
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

        // Track direct signal emission (generalizes has_direct_yield)
        self.current_signal_sources.has_direct_yield = true;

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
                crate::signals::registry::with_registry(|reg| match reg.to_signal_bits(name) {
                    Some(bits) => Ok(bits),
                    None => Err(format!(
                        "{}: emit: unknown signal keyword :{}",
                        syntax.span, name
                    )),
                })
            }
            SyntaxKind::Set(elements) => crate::signals::registry::with_registry(|reg| {
                let mut bits = SignalBits::EMPTY;
                for elem in elements {
                    match &elem.kind {
                        SyntaxKind::Keyword(name) => match reg.to_signal_bits(name) {
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
            }),
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

        // Exhaustiveness check: non-exhaustive match is a compile-time error
        if !arms.is_empty() && !crate::hir::pattern::is_exhaustive_match(&arms) {
            return Err(format!(
                "{}: non-exhaustive match: add a wildcard (_) or variable pattern as the last arm",
                span
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

    /// Analyze a pattern, creating new bindings for variables.
    pub(crate) fn analyze_pattern(&mut self, syntax: &Syntax) -> Result<HirPattern, String> {
        self.analyze_pattern_inner(syntax, &|analyzer, name, scopes, _span| {
            let binding = analyzer.bind(name, scopes, BindingScope::Local);
            Ok(HirPattern::Var(binding))
        })
    }

    /// Analyze a pattern, reusing existing bindings (for or-pattern subsequent alternatives).
    fn analyze_pattern_reuse(&mut self, syntax: &Syntax) -> Result<HirPattern, String> {
        self.analyze_pattern_inner(syntax, &|analyzer, name, scopes, span| {
            let binding = analyzer.lookup(name, scopes).ok_or_else(|| {
                format!(
                    "{}: variable '{}' in or-pattern alternative not bound in first alternative",
                    span, name
                )
            })?;
            Ok(HirPattern::Var(binding))
        })
    }

    /// Core pattern analysis with a callback for variable resolution.
    fn analyze_pattern_inner(
        &mut self,
        syntax: &Syntax,
        resolve_var: &ResolveVar<'_>,
    ) -> Result<HirPattern, String> {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" => Ok(HirPattern::Wildcard),
            SyntaxKind::Symbol(name) if name == "nil" => Ok(HirPattern::Nil),
            SyntaxKind::Symbol(name) => {
                resolve_var(self, name, syntax.scopes.as_slice(), &syntax.span)
            }
            SyntaxKind::Nil => Ok(HirPattern::Nil),
            SyntaxKind::Bool(b) => Ok(HirPattern::Literal(PatternLiteral::Bool(*b))),
            SyntaxKind::Int(n) => Ok(HirPattern::Literal(PatternLiteral::Int(*n))),
            SyntaxKind::Float(f) => Ok(HirPattern::Literal(PatternLiteral::Float(*f))),
            SyntaxKind::String(s) => Ok(HirPattern::Literal(PatternLiteral::String(s.clone()))),
            SyntaxKind::Keyword(k) => Ok(HirPattern::Literal(PatternLiteral::Keyword(k.clone()))),
            SyntaxKind::List(items) => {
                // Or-pattern check FIRST — before any other list pattern logic
                if items
                    .first()
                    .is_some_and(|s| matches!(&s.kind, SyntaxKind::Symbol(name) if name == "or"))
                {
                    return self.analyze_or_pattern(&items[1..], &syntax.span, resolve_var);
                }
                if items.is_empty() {
                    return Ok(HirPattern::List {
                        elements: vec![],
                        rest: None,
                    });
                }
                // Check for cons pattern (head . tail)
                if items.len() == 3 && items[1].as_symbol() == Some(".") {
                    let head = self.analyze_pattern_inner(&items[0], resolve_var)?;
                    let tail = self.analyze_pattern_inner(&items[2], resolve_var)?;
                    return Ok(HirPattern::Pair {
                        head: Box::new(head),
                        tail: Box::new(tail),
                    });
                }
                // Check for dot-rest pattern (a b ... . tail) — 4+ items with "." separator
                if items.len() >= 4 {
                    if let Some(dot_pos) = items.iter().position(|s| s.as_symbol() == Some(".")) {
                        if items.iter().filter(|s| s.as_symbol() == Some(".")).count() > 1 {
                            return Err(format!("{}: multiple '.' in pattern", syntax.span));
                        }
                        if dot_pos != items.len() - 2 {
                            return Err(format!(
                                "{}: '.' must be the second-to-last element in a dotted pattern",
                                syntax.span
                            ));
                        }
                        let fixed = &items[..dot_pos];
                        let rest_syntax = &items[dot_pos + 1];
                        let elements: Result<Vec<_>, _> = fixed
                            .iter()
                            .map(|p| self.analyze_pattern_inner(p, resolve_var))
                            .collect();
                        let rest = self.analyze_pattern_inner(rest_syntax, resolve_var)?;
                        return Ok(HirPattern::List {
                            elements: elements?,
                            rest: Some(Box::new(rest)),
                        });
                    }
                }
                // List pattern with optional & rest
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> = fixed
                    .iter()
                    .map(|p| self.analyze_pattern_inner(p, resolve_var))
                    .collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern_inner(r, resolve_var)?)),
                    None => None,
                };
                Ok(HirPattern::List {
                    elements: elements?,
                    rest,
                })
            }
            SyntaxKind::Array(items) => {
                // Array pattern [...] - matches arrays (immutable)
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> = fixed
                    .iter()
                    .map(|p| self.analyze_pattern_inner(p, resolve_var))
                    .collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern_inner(r, resolve_var)?)),
                    None => None,
                };
                Ok(HirPattern::Tuple {
                    elements: elements?,
                    rest,
                })
            }
            SyntaxKind::ArrayMut(items) => {
                // Array pattern @[...] - matches arrays (mutable)
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> = fixed
                    .iter()
                    .map(|p| self.analyze_pattern_inner(p, resolve_var))
                    .collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern_inner(r, resolve_var)?)),
                    None => None,
                };
                Ok(HirPattern::Array {
                    elements: elements?,
                    rest,
                })
            }
            SyntaxKind::Struct(items) => {
                // Struct pattern {...} - matches structs (immutable)
                let (key_val_items, rest_syntax) = Self::split_struct_rest(items, &syntax.span)?;
                let mut entries = Vec::new();
                for pair in key_val_items.chunks(2) {
                    let key = match &pair[0].kind {
                        SyntaxKind::Keyword(k) => PatternKey::Keyword(k.clone()),
                        SyntaxKind::Quote(inner) => match &inner.kind {
                            SyntaxKind::Symbol(name) => {
                                PatternKey::Symbol(self.symbols.intern(name))
                            }
                            _ => {
                                return Err(format!(
                                "{}: struct pattern key must be a keyword or quoted symbol, got {}",
                                syntax.span, pair[0]
                            ))
                            }
                        },
                        _ => {
                            return Err(format!(
                                "{}: struct pattern key must be a keyword or quoted symbol, got {}",
                                syntax.span, pair[0]
                            ))
                        }
                    };
                    let pattern = self.analyze_pattern_inner(&pair[1], resolve_var)?;
                    entries.push((key, pattern));
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern_inner(r, resolve_var)?)),
                    None => None,
                };
                Ok(HirPattern::Struct { entries, rest })
            }
            SyntaxKind::StructMut(items) => {
                // StructMut pattern @{...} - matches @structs (mutable)
                let (key_val_items, rest_syntax) = Self::split_struct_rest(items, &syntax.span)?;
                let mut entries = Vec::new();
                for pair in key_val_items.chunks(2) {
                    let key = match &pair[0].kind {
                        SyntaxKind::Keyword(k) => PatternKey::Keyword(k.clone()),
                        SyntaxKind::Quote(inner) => match &inner.kind {
                            SyntaxKind::Symbol(name) => {
                                PatternKey::Symbol(self.symbols.intern(name))
                            }
                            _ => {
                                return Err(format!(
                                "{}: struct pattern key must be a keyword or quoted symbol, got {}",
                                syntax.span, pair[0]
                            ))
                            }
                        },
                        _ => {
                            return Err(format!(
                                "{}: struct pattern key must be a keyword or quoted symbol, got {}",
                                syntax.span, pair[0]
                            ))
                        }
                    };
                    let pattern = self.analyze_pattern_inner(&pair[1], resolve_var)?;
                    entries.push((key, pattern));
                }
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern_inner(r, resolve_var)?)),
                    None => None,
                };
                Ok(HirPattern::Table { entries, rest })
            }
            SyntaxKind::Set(items) => {
                // Set pattern |x| - matches sets (immutable)
                if items.len() != 1 {
                    return Err(format!(
                        "{}: set pattern must contain exactly 1 element (the binding pattern)",
                        syntax.span
                    ));
                }
                let binding = self.analyze_pattern_inner(&items[0], resolve_var)?;
                Ok(HirPattern::Set {
                    binding: Box::new(binding),
                })
            }
            SyntaxKind::SetMut(items) => {
                // Mutable set pattern @|x| - matches mutable sets
                if items.len() != 1 {
                    return Err(format!(
                        "{}: mutable set pattern must contain exactly 1 element (the binding pattern)",
                        syntax.span
                    ));
                }
                let binding = self.analyze_pattern_inner(&items[0], resolve_var)?;
                Ok(HirPattern::SetMut {
                    binding: Box::new(binding),
                })
            }
            _ => Err(format!("{}: invalid pattern", syntax.span)),
        }
    }

    /// Analyze an or-pattern: `(or p1 p2 p3)`.
    /// `alternatives` is the slice after the `or` symbol — each element is one pattern.
    fn analyze_or_pattern(
        &mut self,
        alternatives: &[Syntax],
        span: &Span,
        resolve_var: &ResolveVar<'_>,
    ) -> Result<HirPattern, String> {
        use crate::hir::pattern::validate_or_pattern_bindings;

        if alternatives.len() < 2 {
            return Err(format!(
                "{}: or-pattern requires at least two alternatives",
                span
            ));
        }

        let mut patterns = Vec::new();

        // First alternative: use the provided resolve_var (creates bindings in normal mode)
        patterns.push(self.analyze_pattern_inner(&alternatives[0], resolve_var)?);

        // Subsequent alternatives: resolve to existing bindings
        for alt in &alternatives[1..] {
            patterns.push(self.analyze_pattern_reuse(alt)?);
        }

        validate_or_pattern_bindings(&patterns, span, self.arena)?;

        Ok(HirPattern::Or(patterns))
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
