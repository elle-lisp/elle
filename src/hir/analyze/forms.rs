// audited: 2026-11-09
//! Core form analysis: analyze_expr and control flow forms

use super::*;
use crate::hir::expr::CallArg;
use crate::syntax::{Syntax, SyntaxKind};

pub(crate) mod expr;

pub(crate) mod registry;

mod qualified;
mod special;

impl<'a> Analyzer<'a> {
    /// Analyze a quoted datum (`'X` / `(quote X)`) into HIR.
    ///
    /// A quoted COMPOUND datum (list / array / nested structure) becomes a
    /// `HirKind::QuoteConst` carrying a `ConstTemplate` — plain compile-time data
    /// that `MaterializeConst` materializes fresh into a reclaimable region each
    /// execution (region/model.md, "Constants lower as ordinary allocations"). An
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

        // Each branch may skip a `def` the other runs (`merge.rs`).
        self.open_merge();
        let analyzed = self.analyze_branches(&items[1], &items[2], span, |a| match items.get(3) {
            Some(syntax) => a.analyze_expr(syntax),
            None => Ok(Hir::silent(HirKind::Nil, span)),
        });
        self.end_merge(analyzed)
    }

    /// An `If` node inside the merge frame the caller opened, whose else
    /// branch `else_branch` analyzes.
    pub(super) fn analyze_branches(
        &mut self,
        cond: &Syntax,
        then_branch: &Syntax,
        span: Span,
        else_branch: impl FnOnce(&mut Self) -> Result<Hir, String>,
    ) -> Result<Hir, String> {
        let cond = self.analyze_expr(cond)?;
        let cond = self.flush_merges(cond);
        self.merge_branch_point();
        let then_branch = self.analyze_expr(then_branch)?;
        self.merge_branch();
        let else_branch = else_branch(self)?;
        Ok(Self::if_node(cond, then_branch, else_branch, span))
    }

    /// The `If` node over analyzed parts.
    pub(super) fn if_node(cond: Hir, then_branch: Hir, else_branch: Hir, span: Span) -> Hir {
        let signal = cond
            .signal
            .combine(then_branch.signal)
            .combine(else_branch.signal);
        Hir::new(
            HirKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span,
            signal,
        )
    }

    pub(crate) fn analyze_begin(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }
        // `begin` opens no scope, so a body it sits in already owns its `def`
        // forms. One it is the body of owns them here (docs/bindings.md).
        let (exprs, signal) = self.analyze_owned_body(items)?;
        Ok(Hir::new(HirKind::Begin(exprs), span, signal))
    }

    pub(crate) fn analyze_block(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // Check if the first item is a keyword (block name)
        let (name, body_items) = if let Some(first) = items.first() {
            if let SyntaxKind::Keyword(kw) = &first.kind {
                (Some(kw.to_string()), &items[1..])
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
        let result = self.analyze_begin(body_items, span)?;
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
                    (Some(*kw), None)
                } else {
                    (None, Some(&items[0]))
                }
            }
            2 => {
                if let SyntaxKind::Keyword(kw) = &items[0].kind {
                    (Some(*kw), Some(&items[1]))
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
            Hir::silent(HirKind::Nil, span)
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

    /// A body of one form is that form, unless a `def` it owns needs a nil
    /// set before it runs.
    pub(crate) fn analyze_body(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() != 1 {
            return self.analyze_begin(items, span);
        }
        let (mut exprs, signal) = self.analyze_owned_body(items)?;
        if exprs.len() == 1 {
            return Ok(exprs.remove(0));
        }
        Ok(Hir::new(HirKind::Begin(exprs), span, signal))
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

        // A pass may repeat a `def`, and the first may skip it (`merge.rs`).
        // Each pass starts from the binding of each name its body defines
        // that is current at the loop's head.
        let heads = self.open_loop(&items[1..]);
        let analyzed = self.analyze_loop(heads, &items[1], &items[2..], span);
        self.block_contexts.pop();
        let while_node = self.end_merge(analyzed)?;
        let signal = while_node.signal;
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

    /// The `While` node of a loop whose merge frame is open: each pass defines
    /// `heads` before its condition.
    fn analyze_loop(
        &mut self,
        mut heads: Vec<Hir>,
        cond: &Syntax,
        body: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        let cond = self.analyze_expr(cond)?;
        let cond = if heads.is_empty() {
            cond
        } else {
            let signal = cond.signal;
            heads.push(cond);
            Hir::new(HirKind::Begin(heads), span, signal)
        };
        let body = if body.len() == 1 {
            self.analyze_expr(&body[0])?
        } else {
            // Multiple body forms: wrap in implicit begin
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();
            for item in body {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }
            Hir::new(HirKind::Begin(exprs), span, signal)
        };
        let signal = cond.signal.combine(body.signal);
        Ok(Hir::new(
            HirKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_and(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(true), span));
        }
        self.open_merge();
        let analyzed = self.analyze_operands(items);
        let node = analyzed.map(|(exprs, signal)| Hir::new(HirKind::And(exprs), span, signal));
        self.end_merge(node)
    }

    pub(crate) fn analyze_or(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(false), span));
        }
        self.open_merge();
        let analyzed = self.analyze_operands(items);
        let node = analyzed.map(|(exprs, signal)| Hir::new(HirKind::Or(exprs), span, signal));
        self.end_merge(node)
    }

    /// The operands of a short-circuit form, in its open merge frame. Each
    /// operand after the first may not run.
    fn analyze_operands(&mut self, items: &[Syntax]) -> Result<(Vec<Hir>, Signal), String> {
        let mut exprs = Vec::new();
        let mut signal = Signal::silent();
        for item in items {
            let hir = self.analyze_expr(item)?;
            signal = signal.combine(hir.signal);
            exprs.push(hir);
        }
        Ok((exprs, signal))
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
            Hir::silent(HirKind::Nil, span)
        };
        // eval runs the datum on the calling fiber and holds no park of that
        // code, so the one signal it raises is :error: a compile failure, a
        // raise inside the datum, or a yield, an I/O request or a halt the
        // handler refused.
        self.add_inherent_bits(crate::value::SIG_ERROR);
        let signal = Signal::errors().combine(expr.signal).combine(env.signal);
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
