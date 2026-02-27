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
            SyntaxKind::Keyword(k) => Ok(Hir::pure(HirKind::Keyword(k.clone()), span)),

            // Variable reference
            SyntaxKind::Symbol(name) => {
                match self.lookup(name, syntax.scopes.as_slice()) {
                    Some(binding) => Ok(Hir::pure(HirKind::Var(binding), span)),
                    None => {
                        // Treat as global reference
                        let sym = self.symbols.intern(name);
                        let binding = Binding::new(sym, BindingScope::Global);
                        Ok(Hir::pure(HirKind::Var(binding), span))
                    }
                }
            }

            // Tuple literal [...] - call tuple primitive
            SyntaxKind::Tuple(items) => {
                let mut args = Vec::new();
                let mut effect = Effect::none();
                for item in items {
                    let hir = self.analyze_expr(item)?;
                    effect = effect.combine(hir.effect);
                    args.push(hir);
                }
                // Look up the 'tuple' primitive and call it with the elements
                let sym = self.symbols.intern("tuple");
                let binding = Binding::new(sym, BindingScope::Global);
                let func = Hir::new(HirKind::Var(binding), span.clone(), Effect::none());
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

            // Array literal @[...] - call array primitive
            SyntaxKind::Array(items) => {
                let mut args = Vec::new();
                let mut effect = Effect::none();
                for item in items {
                    let hir = self.analyze_expr(item)?;
                    effect = effect.combine(hir.effect);
                    args.push(hir);
                }
                // Look up the 'array' primitive and call it with the elements
                let sym = self.symbols.intern("array");
                let binding = Binding::new(sym, BindingScope::Global);
                let func = Hir::new(HirKind::Var(binding), span.clone(), Effect::none());
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

            // Struct literal {...} - call struct primitive
            SyntaxKind::Struct(items) => {
                let mut args = Vec::new();
                let mut effect = Effect::none();
                for item in items {
                    let hir = self.analyze_expr(item)?;
                    effect = effect.combine(hir.effect);
                    args.push(hir);
                }
                let sym = self.symbols.intern("struct");
                let binding = Binding::new(sym, BindingScope::Global);
                let func = Hir::new(HirKind::Var(binding), span.clone(), Effect::none());
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

            // Table literal @{...} - call table primitive
            SyntaxKind::Table(items) => {
                let mut args = Vec::new();
                let mut effect = Effect::none();
                for item in items {
                    let hir = self.analyze_expr(item)?;
                    effect = effect.combine(hir.effect);
                    args.push(hir);
                }
                let sym = self.symbols.intern("table");
                let binding = Binding::new(sym, BindingScope::Global);
                let func = Hir::new(HirKind::Var(binding), span.clone(), Effect::none());
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

            // Syntax literal — pre-computed Value from macro argument passing
            SyntaxKind::SyntaxLiteral(value) => Ok(Hir::pure(HirKind::Quote(*value), span)),

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
                        "letrec" => return self.analyze_letrec(items, span),
                        "fn" => return self.analyze_lambda(items, span),
                        "begin" => return self.analyze_begin(&items[1..], span),
                        "block" => return self.analyze_block(&items[1..], span),
                        "break" => return self.analyze_break(&items[1..], span),
                        "var" => return self.analyze_define(items, span),
                        "def" => return self.analyze_const(items, span),
                        "set" => return self.analyze_set(items, span),
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
                        "match" => return self.analyze_match(items, span),
                        "cond" => return self.analyze_cond(items, span),
                        "eval" => return self.analyze_eval(items, span),
                        "module" => return self.analyze_module(items, span),
                        "import" => return self.analyze_import(items, span),
                        // (doc <symbol>) → (doc "<symbol-name>")
                        // Rewrites the symbol arg to a string so bare symbols
                        // like (doc if) work without quoting.
                        "doc" if items.len() == 2 => {
                            if let SyntaxKind::Symbol(sym_name) = &items[1].kind {
                                let mut rewritten = items.to_vec();
                                rewritten[1] = Syntax {
                                    kind: SyntaxKind::String(sym_name.clone()),
                                    span: items[1].span.clone(),
                                    scopes: items[1].scopes.clone(),
                                    scope_exempt: items[1].scope_exempt,
                                };
                                return self.analyze_call(&rewritten, span);
                            }
                        }
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
            .combine(then_branch.effect)
            .combine(else_branch.effect);

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
                for (name, scopes) in Self::is_define_form(item) {
                    // Create local binding slot
                    self.bind(name, scopes, BindingScope::Local);
                }
            }

            // Pass 2: Analyze all expressions (all bindings now visible)
            let mut exprs = Vec::new();
            let mut effect = Effect::none();

            for item in items {
                let hir = self.analyze_expr(item)?;
                effect = effect.combine(hir.effect);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, effect))
        } else {
            // At top level, sequential semantics are fine
            let mut exprs = Vec::new();
            let mut effect = Effect::none();

            for item in items {
                let hir = self.analyze_expr(item)?;
                effect = effect.combine(hir.effect);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, effect))
        }
    }

    pub(crate) fn analyze_block(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // Check if the first item is a keyword (block name)
        let (name, body_items) = if let Some(first) = items.first() {
            if let SyntaxKind::Keyword(kw) = &first.kind {
                (Some(kw.clone()), &items[1..])
            } else {
                (None, items)
            }
        } else {
            (None, items)
        };

        let block_id = BlockId(self.next_block_id);
        self.next_block_id += 1;

        self.block_contexts.push(BlockContext {
            block_id,
            name: name.clone(),
            fn_depth: self.fn_depth,
        });

        self.push_scope(false);
        let result = self.analyze_begin(body_items, span.clone())?;
        self.pop_scope();

        self.block_contexts.pop();

        let effect = result.effect;
        Ok(Hir::new(
            HirKind::Block {
                name,
                block_id,
                body: vec![result],
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_break(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // Parse arguments:
        //   (break)           → no name, nil value
        //   (break val)       → no name, has value
        //   (break :name)     → named, nil value
        //   (break :name val) → named, has value
        let (name, value_syntax) = match items.len() {
            0 => (None, None),
            1 => {
                if let SyntaxKind::Keyword(kw) = &items[0].kind {
                    (Some(kw.clone()), None)
                } else {
                    (None, Some(&items[0]))
                }
            }
            2 => {
                if let SyntaxKind::Keyword(kw) = &items[0].kind {
                    (Some(kw.clone()), Some(&items[1]))
                } else {
                    return Err(format!(
                        "{}: break takes at most 2 arguments: optional :name and optional value",
                        span
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "{}: break takes at most 2 arguments: optional :name and optional value",
                    span
                ));
            }
        };

        // Find the target block
        let target = if let Some(ref target_name) = name {
            self.block_contexts
                .iter()
                .rev()
                .find(|ctx| ctx.name.as_deref() == Some(target_name))
                .ok_or_else(|| format!("{}: no block named :{} in scope", span, target_name))?
        } else {
            self.block_contexts
                .last()
                .ok_or_else(|| format!("{}: break outside of any block", span))?
        };

        // Check function boundary
        if target.fn_depth != self.fn_depth {
            return Err(format!("{}: break cannot cross function boundary", span));
        }

        let block_id = target.block_id;

        // Analyze value expression (or nil if absent)
        let value = if let Some(val_syn) = value_syntax {
            self.analyze_expr(val_syn)?
        } else {
            Hir::pure(HirKind::Nil, span.clone())
        };

        let effect = value.effect;
        Ok(Hir::new(
            HirKind::Break {
                block_id,
                value: Box::new(value),
            },
            span,
            effect,
        ))
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

        // Create an implicit named block so `(break :while val)` works
        let block_id = BlockId(self.next_block_id);
        self.next_block_id += 1;

        self.block_contexts.push(BlockContext {
            block_id,
            name: Some("while".to_string()),
            fn_depth: self.fn_depth,
        });

        let cond = self.analyze_expr(&items[1])?;
        let body = self.analyze_expr(&items[2])?;

        self.block_contexts.pop();

        let effect = cond.effect.combine(body.effect);

        let while_node = Hir::new(
            HirKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span.clone(),
            effect,
        );

        Ok(Hir::new(
            HirKind::Block {
                name: Some("while".to_string()),
                block_id,
                body: vec![while_node],
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
        let var_id = self.bind(var_name, items[1].scopes.as_slice(), BindingScope::Local);
        let body = self.analyze_expr(&items[body_idx])?;
        self.pop_scope();

        let effect = iter.effect.combine(body.effect);

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
        let mut effect = Effect::none();

        for item in items {
            let hir = self.analyze_expr(item)?;
            effect = effect.combine(hir.effect);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::And(exprs), span, effect))
    }

    pub(crate) fn analyze_or(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::pure(HirKind::Bool(false), span));
        }

        let mut exprs = Vec::new();
        let mut effect = Effect::none();

        for item in items {
            let hir = self.analyze_expr(item)?;
            effect = effect.combine(hir.effect);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::Or(exprs), span, effect))
    }

    pub(crate) fn analyze_eval(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // (eval expr) or (eval expr env)
        if items.len() < 2 || items.len() > 3 {
            return Err(format!(
                "{}: eval: expected 1 or 2 arguments, got {}",
                span,
                items.len() - 1
            ));
        }
        let expr = self.analyze_expr(&items[1])?;
        let env = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            Hir::pure(HirKind::Nil, span.clone())
        };
        let effect = Effect::yields().combine(expr.effect).combine(env.effect);
        Ok(Hir::new(
            HirKind::Eval {
                expr: Box::new(expr),
                env: Box::new(env),
            },
            span,
            effect,
        ))
    }

    pub(crate) fn analyze_cond(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Ok(Hir::pure(HirKind::Nil, span));
        }

        let mut clauses = Vec::new();
        let mut else_branch = None;
        let mut effect = Effect::none();

        for clause in &items[1..] {
            let parts = clause.as_list().ok_or_else(|| {
                if matches!(clause.kind, SyntaxKind::Tuple(_) | SyntaxKind::Array(_)) {
                    format!(
                        "{}: cond clause must use parentheses (test body...), \
                         not brackets [...]",
                        clause.span
                    )
                } else {
                    format!(
                        "{}: cond clause must be a parenthesized list (test body...), \
                         got {}",
                        clause.span,
                        clause.kind_label()
                    )
                }
            })?;
            if parts.is_empty() {
                continue;
            }

            if parts[0].as_symbol() == Some("else") {
                let body = self.analyze_body(&parts[1..], span.clone())?;
                effect = effect.combine(body.effect);
                else_branch = Some(Box::new(body));
                break;
            }

            let test = self.analyze_expr(&parts[0])?;
            let body = self.analyze_body(&parts[1..], span.clone())?;
            effect = effect.combine(test.effect).combine(body.effect);
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
