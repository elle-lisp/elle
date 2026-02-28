//! Special forms: yield, match

use super::*;
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
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
            let parts = arm.as_list().ok_or_else(|| {
                if matches!(arm.kind, SyntaxKind::Tuple(_) | SyntaxKind::Array(_)) {
                    format!(
                        "{}: match arm must use parentheses (pattern body), \
                         not brackets [...]",
                        arm.span
                    )
                } else {
                    format!(
                        "{}: match arm must be a parenthesized list (pattern body), \
                         got {}",
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

        Ok(Hir::new(
            HirKind::Match {
                value: Box::new(value),
                arms,
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_pattern(&mut self, syntax: &Syntax) -> Result<HirPattern, String> {
        match &syntax.kind {
            SyntaxKind::Symbol(name) if name == "_" => Ok(HirPattern::Wildcard),
            SyntaxKind::Symbol(name) if name == "nil" => Ok(HirPattern::Nil),
            SyntaxKind::Symbol(name) => {
                let binding = self.bind(name, syntax.scopes.as_slice(), BindingScope::Local);
                Ok(HirPattern::Var(binding))
            }
            SyntaxKind::Nil => Ok(HirPattern::Nil),
            SyntaxKind::Bool(b) => Ok(HirPattern::Literal(PatternLiteral::Bool(*b))),
            SyntaxKind::Int(n) => Ok(HirPattern::Literal(PatternLiteral::Int(*n))),
            SyntaxKind::Float(f) => Ok(HirPattern::Literal(PatternLiteral::Float(*f))),
            SyntaxKind::String(s) => Ok(HirPattern::Literal(PatternLiteral::String(s.clone()))),
            SyntaxKind::Keyword(k) => Ok(HirPattern::Literal(PatternLiteral::Keyword(k.clone()))),
            SyntaxKind::List(items) => {
                if items.is_empty() {
                    return Ok(HirPattern::List {
                        elements: vec![],
                        rest: None,
                    });
                }
                // Check for cons pattern (head . tail)
                if items.len() == 3 && items[1].as_symbol() == Some(".") {
                    let head = self.analyze_pattern(&items[0])?;
                    let tail = self.analyze_pattern(&items[2])?;
                    return Ok(HirPattern::Cons {
                        head: Box::new(head),
                        tail: Box::new(tail),
                    });
                }
                // List pattern with optional & rest
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> =
                    fixed.iter().map(|p| self.analyze_pattern(p)).collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern(r)?)),
                    None => None,
                };
                Ok(HirPattern::List {
                    elements: elements?,
                    rest,
                })
            }
            SyntaxKind::Tuple(items) => {
                // Tuple pattern [...] - matches tuples (immutable)
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> =
                    fixed.iter().map(|p| self.analyze_pattern(p)).collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern(r)?)),
                    None => None,
                };
                Ok(HirPattern::Tuple {
                    elements: elements?,
                    rest,
                })
            }
            SyntaxKind::Array(items) => {
                // Array pattern @[...] - matches arrays (mutable)
                let (fixed, rest_syntax) = Self::split_rest_pattern(items, &syntax.span)?;
                let elements: Result<Vec<_>, _> =
                    fixed.iter().map(|p| self.analyze_pattern(p)).collect();
                let rest = match rest_syntax {
                    Some(r) => Some(Box::new(self.analyze_pattern(r)?)),
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
                    let key_name = match &pair[0].kind {
                        SyntaxKind::Keyword(k) => k.clone(),
                        _ => {
                            return Err(format!(
                                "{}: struct pattern key must be a keyword, got {}",
                                syntax.span, pair[0]
                            ))
                        }
                    };
                    let pattern = self.analyze_pattern(&pair[1])?;
                    entries.push((key_name, pattern));
                }
                Ok(HirPattern::Struct { entries })
            }
            SyntaxKind::Table(items) => {
                // Table pattern @{...} - matches tables (mutable)
                if items.len() % 2 != 0 {
                    return Err(format!(
                        "{}: table pattern requires keyword-pattern pairs",
                        syntax.span
                    ));
                }
                let mut entries = Vec::new();
                for pair in items.chunks(2) {
                    let key_name = match &pair[0].kind {
                        SyntaxKind::Keyword(k) => k.clone(),
                        _ => {
                            return Err(format!(
                                "{}: table pattern key must be a keyword, got {}",
                                syntax.span, pair[0]
                            ))
                        }
                    };
                    let pattern = self.analyze_pattern(&pair[1])?;
                    entries.push((key_name, pattern));
                }
                Ok(HirPattern::Table { entries })
            }
            _ => Err(format!("{}: invalid pattern", syntax.span)),
        }
    }
}
