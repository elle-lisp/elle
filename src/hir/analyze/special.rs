//! Special forms: yield, throw, match, handler-case, handler-bind, module, import

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

    pub(crate) fn analyze_throw(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 2 {
            return Err(format!("{}: throw requires 1 argument", span));
        }

        let value = self.analyze_expr(&items[1])?;
        let effect = value.effect.clone();

        Ok(Hir::new(HirKind::Throw(Box::new(value)), span, effect))
    }

    pub(crate) fn analyze_match(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!(
                "{}: match requires value and at least one arm",
                span
            ));
        }

        let value = self.analyze_expr(&items[1])?;
        let mut effect = value.effect.clone();
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
                effect = effect.combine(guard_expr.effect.clone());
                (Some(guard_expr), 3)
            } else {
                (None, 1)
            };

            let body = self.analyze_body(&parts[body_idx..], span.clone())?;
            effect = effect.combine(body.effect.clone());
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

    pub(crate) fn analyze_handler_case(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: handler-case requires a body", span));
        }

        let body = self.analyze_expr(&items[1])?;
        let mut effect = body.effect.clone();
        let mut handlers = Vec::new();

        for handler_syntax in &items[2..] {
            let parts = handler_syntax
                .as_list()
                .ok_or_else(|| format!("{}: handler must be a list", span))?;
            if parts.len() < 3 {
                return Err(format!(
                    "{}: handler requires condition type, var, and body",
                    span
                ));
            }

            // Map condition names to IDs per src/vm/core.rs
            let cond_type = match &parts[0].kind {
                SyntaxKind::Int(n) => *n as u32,
                SyntaxKind::Symbol(s) => {
                    match s.as_str() {
                        "condition" => 1,
                        "error" => 2,
                        "type-error" => 3,
                        "division-by-zero" => 4,
                        "undefined-variable" => 5,
                        "arity-error" => 6,
                        "warning" => 7,
                        "style-warning" => 8,
                        _ => 2, // default to error
                    }
                }
                _ => 2, // default to error
            };

            let var_name = parts[1]
                .as_symbol()
                .ok_or_else(|| format!("{}: handler variable must be a symbol", span))?;

            self.push_scope(false);
            let var_id = self.bind(var_name, BindingKind::Local { index: 0 });
            let handler_body = self.analyze_body(&parts[2..], span.clone())?;
            effect = effect.combine(handler_body.effect.clone());
            self.pop_scope();

            handlers.push((cond_type, var_id, Box::new(handler_body)));
        }

        Ok(Hir::new(
            HirKind::HandlerCase {
                body: Box::new(body),
                handlers,
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_handler_bind(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if items.len() < 2 {
            return Err(format!("{}: handler-bind requires handlers and body", span));
        }

        let handlers_syntax = items[1]
            .as_list()
            .ok_or_else(|| format!("{}: handler-bind handlers must be a list", span))?;

        let mut handlers = Vec::new();
        let mut effect = Effect::pure();

        for handler_syntax in handlers_syntax {
            let parts = handler_syntax
                .as_list()
                .ok_or_else(|| format!("{}: handler must be a list", span))?;
            if parts.len() != 2 {
                return Err(format!(
                    "{}: handler requires condition type and function",
                    span
                ));
            }

            let cond_type = match &parts[0].kind {
                SyntaxKind::Int(n) => *n as u32,
                _ => 0,
            };

            let handler_fn = self.analyze_expr(&parts[1])?;
            effect = effect.combine(handler_fn.effect.clone());
            handlers.push((cond_type, Box::new(handler_fn)));
        }

        let body = self.analyze_body(&items[2..], span.clone())?;
        effect = effect.combine(body.effect.clone());

        Ok(Hir::new(
            HirKind::HandlerBind {
                handlers,
                body: Box::new(body),
            },
            span,
            effect,
        ))
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
        let effect = body.effect.clone();

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
