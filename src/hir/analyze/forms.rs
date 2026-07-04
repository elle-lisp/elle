//! Core form analysis: analyze_expr and control flow forms

use super::*;
use crate::hir::expr::CallArg;
use crate::syntax::{Syntax, SyntaxKind};

mod expr;

pub(crate) mod registry;

mod special;

impl<'a> Analyzer<'a> {
    /// Analyze a quoted datum (`'X` / `(quote X)`) into HIR.
    ///
    /// A quoted COMPOUND datum (list / array / nested structure) becomes a
    /// `HirKind::QuoteConst` carrying a `ConstTemplate` — plain compile-time data
    /// that `MaterializeConst` materializes fresh into a reclaimable region each
    /// execution (region-model.md, "Constants lower as ordinary allocations"). An
    /// IMMEDIATE datum (`'5`, `'foo`, `'()`) stays a `HirKind::Quote` immediate on
    /// the no-region fast path.
    ///
    /// A datum carrying a hygiene-bearing `SyntaxLiteral` (introduced by
    /// quasiquote / macro arg passing, to preserve scope sets) is no exception:
    /// `to_const_template` carries the scope set verbatim in a
    /// `ConstTemplate::SyntaxSymbol`, so it too lowers to an ordinary
    /// `MaterializeConst` allocation, hygiene intact.
    fn analyze_quoted_datum(&mut self, inner: &Syntax, span: Span) -> Result<Hir, String> {
        let template = inner.to_const_template();
        match template.immediate_value(self.symbols) {
            Some(value) => Ok(Hir::silent(HirKind::Quote(value), span)),
            None => Ok(Hir::silent(HirKind::QuoteConst(template), span)),
        }
    }

    /// Resolve a primitive name to its binding via scope lookup.
    ///
    /// Used by collection literal desugaring (Array, ArrayMut, Struct, StructMut)
    /// and qualified symbol desugaring to find the primitive binding
    /// registered by `bind_primitives`. Falls back to a fresh binding
    /// if the name isn't in scope (e.g., in tests without primitives).
    fn resolve_primitive(&mut self, name: &str) -> Binding {
        self.lookup(name, &[]).unwrap_or_else(|| {
            let sym = self.symbols.intern(name);
            self.arena.alloc(sym, BindingScope::Local)
        })
    }

    /// Analyze a %-prefixed intrinsic call.
    fn analyze_intrinsic(
        &mut self,
        name: &str,
        args: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        use crate::hir::expr::IntrinsicOp;

        let op = IntrinsicOp::from_name(name)
            .ok_or_else(|| format!("{}: unknown intrinsic: {}", span, name))?;

        let (min, max) = op.arity();
        if args.len() < min || args.len() > max {
            if min == max {
                return Err(format!(
                    "{}: {} requires exactly {} argument{}, got {}",
                    span,
                    name,
                    min,
                    if min == 1 { "" } else { "s" },
                    args.len()
                ));
            } else {
                return Err(format!(
                    "{}: {} requires {}-{} arguments, got {}",
                    span,
                    name,
                    min,
                    max,
                    args.len()
                ));
            }
        }

        let mut hir_args = Vec::with_capacity(args.len());
        let mut signal = Signal::silent();
        for arg in args {
            let hir = self.analyze_expr(arg)?;
            signal = signal.combine(hir.signal);
            hir_args.push(hir);
        }

        // %-intrinsics are silent — they never yield, error, or perform IO
        Ok(Hir::new(
            HirKind::Intrinsic { op, args: hir_args },
            span,
            signal,
        ))
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
            Hir::silent(HirKind::Nil, span.clone())
        };

        let signal = cond
            .signal
            .combine(then_branch.signal)
            .combine(else_branch.signal);

        Ok(Hir::new(
            HirKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_begin(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        // Check if we're inside a function scope
        let in_function = self.scopes.iter().any(|s| s.is_function);

        if in_function {
            // Two-pass analysis for letrec-style semantics:
            // Pass 1: Create bindings for all defines (without analyzing values)
            //
            // A fn body is a letrec* context (docs/bindings.md "Function
            // bodies are an implicit letrec"): defining the same binding
            // identity twice in one body is rejected exactly like an
            // explicit letrec's duplicate.
            let mut duplicates = super::scopes::DuplicateGuard::default();
            for item in items {
                for (name, scopes) in Self::is_define_form(item) {
                    // Create local binding slot, marked prebound so that
                    // needs_capture() knows the binding may be captured before
                    // its initializer runs (self-recursion, forward refs).
                    let sym = self.symbols.intern(name);
                    duplicates.check(sym, name, scopes, &item.span)?;
                    let binding = self.bind(name, scopes, BindingScope::Local);
                    let fn_depth = self.fn_depth;
                    let inner = self.arena.get_mut(binding);
                    inner.is_prebound = true;
                    inner.init_pending = true;
                    inner.prebind_fn_depth = fn_depth;
                }
            }

            // Pass 2: Analyze all expressions (all bindings now visible)
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();

            for item in items {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, signal))
        } else {
            // At top level, sequential semantics are fine
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();

            for item in items {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, signal))
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

        let signal = result.signal;
        Ok(Hir::new(
            HirKind::Block {
                name,
                block_id,
                body: vec![result],
            },
            span,
            signal,
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
            Hir::silent(HirKind::Nil, span.clone())
        };

        let signal = value.signal;
        Ok(Hir::new(
            HirKind::Break {
                block_id,
                value: Box::new(value),
            },
            span,
            signal,
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
        if items.len() < 3 {
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
        let body = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            // Multiple body forms: wrap in implicit begin
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();
            for item in &items[2..] {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }
            Hir::new(HirKind::Begin(exprs), span.clone(), signal)
        };

        self.block_contexts.pop();

        let signal = cond.signal.combine(body.signal);

        let while_node = Hir::new(
            HirKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span.clone(),
            signal,
        );

        Ok(Hir::new(
            HirKind::Block {
                name: Some("while".to_string()),
                block_id,
                body: vec![while_node],
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_and(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(true), span));
        }

        let mut exprs = Vec::new();
        let mut signal = Signal::silent();

        for item in items {
            let hir = self.analyze_expr(item)?;
            signal = signal.combine(hir.signal);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::And(exprs), span, signal))
    }

    pub(crate) fn analyze_or(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(false), span));
        }

        let mut exprs = Vec::new();
        let mut signal = Signal::silent();

        for item in items {
            let hir = self.analyze_expr(item)?;
            signal = signal.combine(hir.signal);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::Or(exprs), span, signal))
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
            Hir::silent(HirKind::Nil, span.clone())
        };
        let signal = Signal::yields().combine(expr.signal).combine(env.signal);
        Ok(Hir::new(
            HirKind::Eval {
                expr: Box::new(expr),
                env: Box::new(env),
            },
            span,
            signal,
        ))
    }
}
