//! Special forms: yield, match, restrict

use super::*;
use crate::effects::registry;
use crate::hir::pattern::{HirPattern, PatternKey, PatternLiteral};
use crate::syntax::{Syntax, SyntaxKind};

/// Callback type for resolving variable patterns.
/// In normal mode, creates new bindings. In or-pattern reuse mode, looks up existing bindings.
type ResolveVar<'a> =
    dyn Fn(&mut Analyzer<'_>, &str, &[ScopeId], &Span) -> Result<HirPattern, String> + 'a;

impl<'a> Analyzer<'a> {
    /// Analyze a `(restrict ...)` form.
    ///
    /// restrict is a declaration, not an expression. It must appear inside
    /// a lambda body. It accumulates into `current_param_bounds` and
    /// `current_declared_ceiling`, which `analyze_lambda` reads after
    /// analyzing the body.
    ///
    /// Forms:
    /// - `(restrict)` — function-level ceiling = inert
    /// - `(restrict :kw ...)` — function-level ceiling with specific signals
    /// - `(restrict param)` — parameter bound = inert
    /// - `(restrict param :kw ...)` — parameter bound with specific signals
    pub(crate) fn analyze_restrict(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if self.fn_depth == 0 {
            return Err(format!(
                "{}: restrict must appear inside a function body",
                span
            ));
        }

        let args = &items[1..];
        if args.is_empty() {
            // (restrict) — function-level ceiling = inert
            self.current_declared_ceiling = Some(Effect::inert());
            return Ok(Hir::inert(HirKind::Nil, span));
        }

        match &args[0].kind {
            SyntaxKind::Keyword(_) => {
                // (restrict :kw1 :kw2 ...) — function-level ceiling
                let mut bits = 0u32;
                for arg in args {
                    let kw = match &arg.kind {
                        SyntaxKind::Keyword(k) => k,
                        _ => {
                            return Err(format!(
                                "{}: restrict: expected keyword, got {}",
                                arg.span,
                                arg.kind_label()
                            ));
                        }
                    };
                    let reg = registry::global_registry().lock().unwrap();
                    let bit_pos = reg.lookup(kw).ok_or_else(|| {
                        format!(
                            "{}: restrict: effect :{} not registered (unknown effect keyword)",
                            arg.span, kw
                        )
                    })?;
                    bits |= 1 << bit_pos;
                }
                self.current_declared_ceiling = Some(Effect {
                    bits: crate::value::fiber::SignalBits(bits),
                    propagates: 0,
                });
            }
            SyntaxKind::Symbol(param_name) => {
                // (restrict param :kw1 :kw2 ...) — parameter-level bound
                let binding = self.find_current_param_binding(param_name, &args[0].span)?;

                let keywords = &args[1..];
                let bound = if keywords.is_empty() {
                    // (restrict param) — bound is inert
                    Effect::inert()
                } else {
                    let mut bits = 0u32;
                    for kw_syntax in keywords {
                        let kw = match &kw_syntax.kind {
                            SyntaxKind::Keyword(k) => k,
                            _ => {
                                return Err(format!(
                                    "{}: restrict: expected keyword after parameter name, got {}",
                                    kw_syntax.span,
                                    kw_syntax.kind_label()
                                ));
                            }
                        };
                        let reg = registry::global_registry().lock().unwrap();
                        let bit_pos = reg.lookup(kw).ok_or_else(|| {
                            format!(
                                "{}: restrict: effect :{} not registered (unknown effect keyword)",
                                kw_syntax.span, kw
                            )
                        })?;
                        bits |= 1 << bit_pos;
                    }
                    Effect {
                        bits: crate::value::fiber::SignalBits(bits),
                        propagates: 0,
                    }
                };

                // Last wins for duplicate parameter restricts
                self.current_param_bounds.insert(binding, bound);
            }
            _ => {
                return Err(format!(
                    "{}: restrict: expected keyword or parameter name, got {}",
                    args[0].span,
                    args[0].kind_label()
                ));
            }
        }

        Ok(Hir::inert(HirKind::Nil, span))
    }

    /// Find a parameter binding by name in the current lambda's params.
    fn find_current_param_binding(&self, name: &str, span: &Span) -> Result<Binding, String> {
        for param in &self.current_lambda_params {
            if self.symbols.name(param.name()) == Some(name) {
                return Ok(*param);
            }
        }
        Err(format!(
            "{}: restrict: '{}' is not a parameter of this function",
            span, name
        ))
    }

    pub(crate) fn analyze_yield(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 2 {
            return Err(format!("{}: yield requires 1 argument", span));
        }

        let value = self.analyze_expr(&items[1])?;

        // Track that this lambda has a direct yield (not from calling a parameter)
        self.current_effect_sources.has_direct_yield = true;

        Ok(Hir::new(
            HirKind::Yield(Box::new(value)),
            span,
            Effect::yields(), // Yield always has Yields effect
        ))
    }

    pub(crate) fn analyze_match(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!(
                "{}: match requires value and at least one arm",
                span
            ));
        }

        let value = self.analyze_expr(&items[1])?;
        let mut effect = value.effect;
        let mut arms = Vec::new();

        for arm in &items[2..] {
            let parts = arm.as_list_or_tuple().ok_or_else(|| {
                if matches!(arm.kind, SyntaxKind::ArrayMut(_)) {
                    format!(
                        "{}: match arm must use (...) or [...], not @[...]",
                        arm.span
                    )
                } else {
                    format!(
                        "{}: match arm must be a list (...) or [...], got {}",
                        arm.span,
                        arm.kind_label()
                    )
                }
            })?;
            if parts.len() < 2 {
                return Err(format!("{}: match arm requires pattern and body", span));
            }

            self.push_scope(false);
            let pattern = self.analyze_pattern(&parts[0])?;

            // Check for guard
            let (guard, body_idx) = if parts.len() >= 3 && parts[1].as_symbol() == Some("when") {
                let guard_expr = self.analyze_expr(&parts[2])?;
                effect = effect.combine(guard_expr.effect);
                (Some(guard_expr), 3)
            } else {
                (None, 1)
            };

            let body = self.analyze_body(&parts[body_idx..], span.clone())?;
            effect = effect.combine(body.effect);
            self.pop_scope();

            arms.push((pattern, guard, body));
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
            effect,
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
                    return Ok(HirPattern::Cons {
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
                if items.len() % 2 != 0 {
                    return Err(format!(
                        "{}: struct pattern requires keyword-pattern pairs",
                        syntax.span
                    ));
                }
                let mut entries = Vec::new();
                for pair in items.chunks(2) {
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
                Ok(HirPattern::Struct { entries })
            }
            SyntaxKind::StructMut(items) => {
                // StructMut pattern @{...} - matches @structs (mutable)
                if items.len() % 2 != 0 {
                    return Err(format!(
                        "{}: struct pattern requires keyword-pattern pairs",
                        syntax.span
                    ));
                }
                let mut entries = Vec::new();
                for pair in items.chunks(2) {
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
                Ok(HirPattern::Table { entries })
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

        validate_or_pattern_bindings(&patterns, span)?;

        Ok(HirPattern::Or(patterns))
    }
}
