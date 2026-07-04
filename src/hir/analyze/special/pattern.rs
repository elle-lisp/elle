//! Pattern analysis for match forms.

use super::*;
use crate::hir::pattern::{HirPattern, PatternKey, PatternLiteral};
use crate::syntax::{Syntax, SyntaxKind};

/// Callback type for resolving variable patterns.
/// In normal mode, creates new bindings. In or-pattern reuse mode, looks up existing bindings.
type ResolveVar<'a> =
    dyn Fn(&mut Analyzer<'_>, &str, &[ScopeId], &Span) -> Result<HirPattern, String> + 'a;

impl<'a> Analyzer<'a> {
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
}
