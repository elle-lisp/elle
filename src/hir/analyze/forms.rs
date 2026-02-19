//! Core form analysis: analyze_expr and control flow forms

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_expr(&mut self, syntax: &Syntax) -> Result<Hir, String> {
        let span = syntax.span.clone();

        match &syntax.kind {
            // Literals
            SyntaxKind::Nil => Ok(Hir::pure(HirKind::Nil, span)),
            SyntaxKind::Bool(b) => Ok(Hir::pure(HirKind::Bool(*b), span)),
            SyntaxKind::Int(n) => Ok(Hir::pure(HirKind::Int(*n), span)),
            SyntaxKind::Float(f) => Ok(Hir::pure(HirKind::Float(*f), span)),
            SyntaxKind::String(s) => Ok(Hir::pure(HirKind::String(s.clone()), span)),
            SyntaxKind::Keyword(k) => {
                let sym = self.symbols.intern(k);
                Ok(Hir::pure(HirKind::Keyword(sym), span))
            }

            // Variable reference
            SyntaxKind::Symbol(name) => {
                match self.lookup(name) {
                    Some(id) => Ok(Hir::pure(HirKind::Var(id), span)),
                    None => {
                        // Treat as global reference
                        let sym = self.symbols.intern(name);
                        let id = self.ctx.fresh_binding();
                        self.ctx.register_binding(BindingInfo::global(id, sym));
                        Ok(Hir::pure(HirKind::Var(id), span))
                    }
                }
            }

            // Vector literal - call vector primitive
            SyntaxKind::Vector(items) => {
                let mut args = Vec::new();
                let mut effect = Effect::Pure;
                for item in items {
                    let hir = self.analyze_expr(item)?;
                    effect = effect.combine(hir.effect.clone());
                    args.push(hir);
                }
                // Look up the 'vector' primitive and call it with the elements
                let sym = self.symbols.intern("vector");
                let id = self.ctx.fresh_binding();
                self.ctx.register_binding(BindingInfo::global(id, sym));
                let func = Hir::new(HirKind::Var(id), span.clone(), Effect::Pure);
                Ok(Hir::new(
                    HirKind::Call {
                        func: Box::new(func),
                        args,
                        is_tail: false,
                    },
                    span,
                    effect,
                ))
            }

            // Quote - convert to Value at analysis time
            SyntaxKind::Quote(inner) => {
                let value = (**inner).to_value(self.symbols);
                Ok(Hir::pure(HirKind::Quote(value), span))
            }

            // Quasiquote, Unquote, UnquoteSplicing should have been expanded
            SyntaxKind::Quasiquote(_) | SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                Err(format!(
                    "{}: quasiquote forms should be expanded before analysis",
                    span
                ))
            }

            // List - could be special form or function call
            SyntaxKind::List(items) => {
                if items.is_empty() {
                    return Ok(Hir::pure(HirKind::EmptyList, span));
                }

                // Check for special forms
                if let SyntaxKind::Symbol(name) = &items[0].kind {
                    match name.as_str() {
                        "if" => return self.analyze_if(items, span),
                        "let" => return self.analyze_let(items, span),
                        "let*" => return self.analyze_let_star(items, span),
                        "letrec" => return self.analyze_letrec(items, span),
                        "fn" | "lambda" => return self.analyze_lambda(items, span),
                        "begin" => return self.analyze_begin(&items[1..], span),
                        "block" => return self.analyze_block(&items[1..], span),
                        "define" => return self.analyze_define(items, span),
                        "set!" => return self.analyze_set(items, span),
                        "while" => return self.analyze_while(items, span),
                        "each" => return self.analyze_for(items, span),
                        "and" => return self.analyze_and(&items[1..], span),
                        "or" => return self.analyze_or(&items[1..], span),
                        "quote" => {
                            if items.len() != 2 {
                                return Err(format!("{}: quote requires 1 argument", span));
                            }
                            let value = items[1].to_value(self.symbols);
                            return Ok(Hir::pure(HirKind::Quote(value), span));
                        }
                        "yield" => return self.analyze_yield(items, span),
                        "throw" => return self.analyze_throw(items, span),
                        "match" => return self.analyze_match(items, span),
                        "cond" => return self.analyze_cond(items, span),
                        "handler-case" => return self.analyze_handler_case(items, span),
                        "handler-bind" => return self.analyze_handler_bind(items, span),
                        "module" => return self.analyze_module(items, span),
                        "import" => return self.analyze_import(items, span),
                        _ => {}
                    }
                }

                // Regular function call
                self.analyze_call(items, span)
            }
        }
    }

    pub(crate) fn analyze_if(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 || items.len() > 4 {
            return Err(format!("{}: if requires 2 or 3 arguments", span));
        }

        let cond = self.analyze_expr(&items[1])?;
        let then_branch = self.analyze_expr(&items[2])?;
        let else_branch = if items.len() == 4 {
            self.analyze_expr(&items[3])?
        } else {
            Hir::pure(HirKind::Nil, span.clone())
        };

        let effect = cond
            .effect
            .clone()
            .combine(then_branch.effect.clone())
            .combine(else_branch.effect.clone());

        Ok(Hir::new(
            HirKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_begin(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::pure(HirKind::Nil, span));
        }

        // Check if we're inside a function scope
        let in_function = self.scopes.iter().any(|s| s.is_function);

        if in_function {
            // Two-pass analysis for letrec-style semantics:
            // Pass 1: Create bindings for all defines (without analyzing values)
            for item in items {
                if let Some(name) = Self::is_define_form(item) {
                    // Create local binding slot
                    let local_index = self.current_local_count();
                    self.bind(name, BindingKind::Local { index: local_index });
                }
            }

            // Pass 2: Analyze all expressions (all bindings now visible)
            let mut exprs = Vec::new();
            let mut effect = Effect::Pure;

            for item in items {
                let hir = self.analyze_expr(item)?;
                effect = effect.combine(hir.effect.clone());
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, effect))
        } else {
            // At top level, sequential semantics are fine
            let mut exprs = Vec::new();
            let mut effect = Effect::Pure;

            for item in items {
                let hir = self.analyze_expr(item)?;
                effect = effect.combine(hir.effect.clone());
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, effect))
        }
    }

    pub(crate) fn analyze_block(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        self.push_scope(false);
        let result = self.analyze_begin(items, span.clone())?;
        self.pop_scope();

        let effect = result.effect.clone();
        Ok(Hir::new(HirKind::Block(vec![result]), span, effect))
    }

    pub(crate) fn analyze_body(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() == 1 {
            self.analyze_expr(&items[0])
        } else {
            self.analyze_begin(items, span)
        }
    }

    pub(crate) fn analyze_while(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 3 {
            return Err(format!("{}: while requires condition and body", span));
        }

        let cond = self.analyze_expr(&items[1])?;
        let body = self.analyze_expr(&items[2])?;
        let effect = cond.effect.clone().combine(body.effect.clone());

        Ok(Hir::new(
            HirKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_for(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // (each var iter body) or (each var in iter body)
        if items.len() < 4 {
            return Err(format!(
                "{}: each requires variable, iterator, and body",
                span
            ));
        }

        let var_name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: each variable must be a symbol", span))?;

        // Check for optional 'in' keyword
        let (iter_idx, body_idx) = if items.len() == 5 {
            if items[2].as_symbol() == Some("in") {
                (3, 4)
            } else {
                return Err(format!(
                    "{}: each syntax is (each var iter body) or (each var in iter body)",
                    span
                ));
            }
        } else {
            (2, 3)
        };

        let iter = self.analyze_expr(&items[iter_idx])?;

        self.push_scope(false);
        let var_id = self.bind(var_name, BindingKind::Local { index: 0 });
        let body = self.analyze_expr(&items[body_idx])?;
        self.pop_scope();

        let effect = iter.effect.clone().combine(body.effect.clone());

        Ok(Hir::new(
            HirKind::For {
                var: var_id,
                iter: Box::new(iter),
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_and(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::pure(HirKind::Bool(true), span));
        }

        let mut exprs = Vec::new();
        let mut effect = Effect::Pure;

        for item in items {
            let hir = self.analyze_expr(item)?;
            effect = effect.combine(hir.effect.clone());
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::And(exprs), span, effect))
    }

    pub(crate) fn analyze_or(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::pure(HirKind::Bool(false), span));
        }

        let mut exprs = Vec::new();
        let mut effect = Effect::Pure;

        for item in items {
            let hir = self.analyze_expr(item)?;
            effect = effect.combine(hir.effect.clone());
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::Or(exprs), span, effect))
    }

    pub(crate) fn analyze_cond(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Ok(Hir::pure(HirKind::Nil, span));
        }

        let mut clauses = Vec::new();
        let mut else_branch = None;
        let mut effect = Effect::Pure;

        for clause in &items[1..] {
            let parts = clause
                .as_list()
                .ok_or_else(|| format!("{}: cond clause must be a list", span))?;
            if parts.is_empty() {
                continue;
            }

            if parts[0].as_symbol() == Some("else") {
                let body = self.analyze_body(&parts[1..], span.clone())?;
                effect = effect.combine(body.effect.clone());
                else_branch = Some(Box::new(body));
                break;
            }

            let test = self.analyze_expr(&parts[0])?;
            let body = self.analyze_body(&parts[1..], span.clone())?;
            effect = effect
                .combine(test.effect.clone())
                .combine(body.effect.clone());
            clauses.push((test, body));
        }

        Ok(Hir::new(
            HirKind::Cond {
                clauses,
                else_branch,
            },
            span,
            effect,
        ))
    }
}
