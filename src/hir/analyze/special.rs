//! Special forms: yield, match, module, import

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
            let parts = arm
                .as_list()
                .ok_or_else(|| format!("{}: match arm must be a list", span))?;
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
                let id = self.bind(
                    name,
                    BindingKind::Local {
                        index: self.current_local_index(),
                    },
                );
                Ok(HirPattern::Var(id))
            }
            SyntaxKind::Nil => Ok(HirPattern::Nil),
            SyntaxKind::Bool(b) => Ok(HirPattern::Literal(PatternLiteral::Bool(*b))),
            SyntaxKind::Int(n) => Ok(HirPattern::Literal(PatternLiteral::Int(*n))),
            SyntaxKind::Float(f) => Ok(HirPattern::Literal(PatternLiteral::Float(*f))),
            SyntaxKind::String(s) => Ok(HirPattern::Literal(PatternLiteral::String(s.clone()))),
            SyntaxKind::Keyword(k) => {
                let sym = self.symbols.intern(k);
                Ok(HirPattern::Literal(PatternLiteral::Keyword(sym)))
            }
            SyntaxKind::List(items) => {
                if items.is_empty() {
                    return Ok(HirPattern::List(vec![]));
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
                // List pattern
                let patterns: Result<Vec<_>, _> =
                    items.iter().map(|p| self.analyze_pattern(p)).collect();
                Ok(HirPattern::List(patterns?))
            }
            SyntaxKind::Vector(items) => {
                let patterns: Result<Vec<_>, _> =
                    items.iter().map(|p| self.analyze_pattern(p)).collect();
                Ok(HirPattern::Vector(patterns?))
            }
            _ => Err(format!("{}: invalid pattern", syntax.span)),
        }
    }

    pub(crate) fn analyze_module(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: module requires a name", span));
        }

        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: module name must be a symbol", span))?;
        let name_sym = self.symbols.intern(name);

        let mut exports = Vec::new();
        let mut body_start = 2;

        // Check for :export clause
        if items.len() > 2 {
            if let SyntaxKind::Keyword(kw) = &items[2].kind {
                if kw == "export" && items.len() > 3 {
                    let export_list = items[3]
                        .as_list()
                        .ok_or_else(|| format!("{}: :export must be followed by a list", span))?;
                    for exp in export_list {
                        let exp_name = exp
                            .as_symbol()
                            .ok_or_else(|| format!("{}: export must be a symbol", span))?;
                        exports.push(self.symbols.intern(exp_name));
                    }
                    body_start = 4;
                }
            }
        }

        let body = self.analyze_body(&items[body_start..], span.clone())?;
        let effect = body.effect;

        Ok(Hir::new(
            HirKind::Module {
                name: name_sym,
                exports,
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_import(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 2 {
            return Err(format!("{}: import requires a module name", span));
        }

        let module = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: import module must be a symbol", span))?;
        let module_sym = self.symbols.intern(module);

        Ok(Hir::pure(HirKind::Import { module: module_sym }, span))
    }
}
