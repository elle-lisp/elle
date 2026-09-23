// audited: 2026-09-22
//! A-normal form (ANF) lift: every heap-allocating value gets a binding whose
//! slot the lowerer releases it through.
//!
//! docs/impl/hir.md
//! docs/impl/region/rules.md
//!
//! Names every allocating expression by wrapping it in a synthetic
//! `let` whose body is the bound variable. After this pass, every
//! heap-allocating value this frame releases through a slot has a
//! `Binding` — meaning the lowerer can key slot ownership entirely off
//! `binding_to_slot`, with no shadow mechanism for un-named call results.
//!
//! Example rewrite:
//!
//! ```text
//! (g (f x))    =>    (g (let [t0 (f x)] t0))
//! ```
//!
//! `t0` is a synthetic immutable binding. Region inference (which runs
//! after ANF) sees `f`'s call result as bound to `t0`, so escape
//! analysis owns its lifetime through a single mechanism.
//!
//! Pipeline placement: immediately after `functionalize`, before
//! `typeinfer` and region analysis.
//!
//! ## The name has to land on the node that allocates
//!
//! A name is worth exactly what the lowerer can key off it, and what the lowerer
//! keys off it is one map: `record_region_slot` records a binder's slot against
//! `alloc_region[init.id]` — the region the init node **itself** allocates. A
//! binder whose init allocates nothing at its own id therefore records nothing,
//! its value gets no release route, and the release the solver placed for it
//! emits no instruction at all.
//!
//! So "name the allocating value" is a claim about a node, not about a position.
//! Every position below is read that way.
//!
//! ## What gets wrapped
//!
//! The traversal recurses through every child. After the recursive
//! call returns, the parent decides whether to wrap based on the
//! child's position:
//!
//! **Consumer positions (wrap allocating children):**
//! `Call.func` and `Call.args[*].expr`; `Intrinsic.args[*]`;
//! `Emit.value`; `Recur.args[*]`; `Eval.{expr, env}`;
//! `Parameterize.bindings[*].{key, value}`; `If.{cond, then, else}`;
//! `Cond` clauses (cond and body); `Match.value` and arm bodies;
//! every `Begin` expression and every `Block.body` expression
//! (both last and non-last — non-last positions discard the value,
//! and the binding's slot is what `emit_decrefs_for` uses to release
//! the call result region); `And`/`Or` elements; `Break.value`;
//! `SetCell.{cell, value}`; `Assign.value`; `Destructure.value`;
//! `While.{cond, body}`.
//!
//! **Binder positions (the binder's own slot is the name):**
//! `Let` / `Letrec` / `Loop` binding RHS; `Define.value`. An init that allocates
//! at its own id is recorded against that binder's slot, so wrapping it would
//! chain a second name for one region.
//!
//! **Transparent in the lowerer (do NOT wrap):**
//! `MakeCell.value`, `DerefCell.cell`. The lowerer is transparent for
//! these and the implicit `MakeCaptureCell` happens at the binding
//! site; wrapping their child manufactures a region with no matching
//! allocation.
//!
//! **Returning positions (name only an owed release):**
//! `Lambda.body`, and the root of the compilation unit. See the section
//! below.
//!
//! ## A propagating tail is named through, never named
//!
//! A `Let`, `Letrec`, `Loop` or `Parameterize` hands its **body's** value up
//! unchanged, and the lowerer stamps no allocation at the form's own id. The
//! form is therefore the wrong node to name: a wrap around it binds a slot that
//! `record_region_slot` leaves empty, and a binder that already holds it —
//! `(let [a (let [x …] [x x])] …)` — holds a name with no route.
//!
//! So both naming positions descend the tail and name the node they find there.
//! A consumer position wraps that node; a binder position names it too, because
//! the binder's own slot cannot stand for a region the init node did not
//! allocate. `(g (let [x 7] [x x]))` becomes `(g (let [x 7] (let [t [x x]] t)))`,
//! and the inner walk a fused `mapcat` runs over its function's result reaches
//! its per-element array the same way (docs/impl/dissolution.md).
//!
//! ## A returning position names only what it must release
//!
//! A lambda body and the root of a compilation unit are returning positions.
//! Their value leaves through the `Return` mint (`return_incref.rs`), which
//! hands the caller one owning reference. Most values there need no name: a
//! tail call hands its callee's reference straight through, and a fresh
//! allocation's region has a release of its own.
//!
//! Two producers hand this frame an owning reference that only a slot can
//! release: an `Eval`, which is never a tail call, and a `Call` that is not a
//! tail call. The root makes the second kind, because `mark_tail_calls` marks
//! no call at the top level. A `parameterize` body makes it too, because that
//! body is never a tail position. Left unnamed, the frame's reference is never
//! released, and the `Return` mint adds the caller's on top of it.
//!
//! So a returning position descends its propagating tails, as a consumer
//! does, and names an `Eval` or a non-tail `Call` it finds there. It names
//! nothing else, so a tail call stays where it is, unnamed.
//!
//! ## Idempotence
//!
//! If a child is already an ANF wrap `(let [t e] (var t))`, the
//! parent does not re-wrap it. Re-wrapping would chain a redundant
//! synthetic binding and confuse `region_to_slot` (which keys on the
//! region) with two slots claiming the same region.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirKind};

/// Run the ANF lift on a HIR tree.
///
/// When `--anf=off` is set on the CLI, this is a no-op.
pub fn anf_lift(hir: &mut Hir, arena: &mut BindingArena) {
    if !crate::config::get().anf {
        return;
    }
    let mut ctx = AnfCtx { arena };
    *hir = ctx.transform(hir);
    // After ANF, tail positions are settled. Mark each function's tail
    // value with a `Return` ownership boundary (the callee side of the
    // prediction-free calling convention).
    super::return_incref::wrap_tail_returns(hir);
}

struct AnfCtx<'a> {
    arena: &'a mut BindingArena,
}

impl<'a> AnfCtx<'a> {
    /// Generate a fresh immutable synthetic binding for an ANF temp.
    fn gensym(&mut self) -> Binding {
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_immutable = true;
        b
    }

    /// If `hir.allocates()` and isn't already an ANF wrap, name it
    /// by wrapping in `Let([gensym, hir], Var(gensym))`. The wrap's
    /// `Hir::span` and `Hir::signal` reuse the child's so the
    /// synthetic Let's diagnostics still point at the original site.
    ///
    /// A propagating tail is descended rather than wrapped: the node that
    /// allocates is the one a name is worth anything on.
    fn name_if_alloc(&mut self, mut hir: Hir) -> Hir {
        if is_anf_wrapped(&hir) {
            return hir;
        }
        if self.name_tail(&mut hir) {
            return hir;
        }
        if !hir.allocates() {
            return hir;
        }
        let span = hir.span;
        let signal = hir.signal;
        let b = self.gensym();
        let var = Hir::silent(HirKind::Var(b), span);
        if crate::config::get().trace_bits() & crate::config::trace_bits::ANF != 0 {
            eprintln!(
                "[trace:anf] wrap {} @{:?} (span={})",
                kind_label(&hir.kind),
                hir.id,
                span,
            );
        }
        Hir::new(
            HirKind::Let {
                bindings: vec![(b, hir)],
                body: Box::new(var),
            },
            span,
            signal,
        )
    }

    /// Name the value a propagating tail hands up, in place. Answers whether
    /// `hir` was one — the caller has nothing left to name when it was, the
    /// form itself allocating nothing.
    ///
    /// The descent is recursive because a tail can hand up another tail:
    /// `(let [a 1] (let [b 2] (f b)))` puts the call two forms down.
    fn name_tail(&mut self, hir: &mut Hir) -> bool {
        let span = hir.span;
        let Some(tail) = hir.propagating_tail_mut() else {
            return false;
        };
        let inner = std::mem::replace(tail, Hir::silent(HirKind::Nil, span));
        *tail = self.name_if_alloc(inner);
        true
    }

    /// Transform a child in a NON-WRAP position: recurse into its
    /// own children but do not wrap the resulting node at this level.
    /// Used for MakeCell/DerefCell pass-through children, a lambda body, and
    /// propagating tail bodies (their own consumer descends into them).
    fn t(&mut self, hir: &Hir) -> Hir {
        self.transform(hir)
    }

    /// Transform a child in a BINDER position — a `let`/`letrec`/`loop`
    /// binding's value, or a `def`'s. The binder's own slot names an init that
    /// allocates at its own id, so the form is never wrapped; a propagating
    /// tail's value is allocated by a node that slot cannot stand for, so the
    /// tail is named through exactly as a consumer names it.
    fn b(&mut self, hir: &Hir) -> Hir {
        let mut inner = self.transform(hir);
        self.name_tail(&mut inner);
        inner
    }

    /// Transform a child in a WRAP position: recurse, then wrap if
    /// the result allocates.
    fn w(&mut self, hir: &Hir) -> Hir {
        let inner = self.transform(hir);
        self.name_if_alloc(inner)
    }

    /// Bottom-up rewrite. Visits each `HirKind` and decides per
    /// child position whether to wrap.
    fn transform(&mut self, hir: &Hir) -> Hir {
        let new_kind = match &hir.kind {
            // ── Leaves: no children ──
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Var(_)
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_)
            | HirKind::Error => hir.kind.clone(),

            // ── Binding forms: the binder's own slot is the RHS's name ──
            HirKind::Let { bindings, body } => HirKind::Let {
                bindings: bindings
                    .iter()
                    .map(|(binding, init)| (*binding, self.b(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },
            HirKind::Letrec { bindings, body } => HirKind::Letrec {
                bindings: bindings
                    .iter()
                    .map(|(binding, init)| (*binding, self.b(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },
            HirKind::Loop { bindings, body } => HirKind::Loop {
                bindings: bindings
                    .iter()
                    .map(|(binding, init)| (*binding, self.b(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },

            // ── Define: the binding's slot names the value ──
            HirKind::Define { binding, value } => HirKind::Define {
                binding: *binding,
                value: Box::new(self.b(value)),
            },

            // ── Lambda body: propagating tail; do not wrap ──
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body,
                num_locals,
                inferred_signals,
                param_bounds,
                doc,
                origin,
                assert_numeric,
            } => HirKind::Lambda {
                params: params.clone(),
                num_required: *num_required,
                rest_param: *rest_param,
                vararg_kind: vararg_kind.clone(),
                captures: captures.clone(),
                body: Box::new(self.t(body)),
                num_locals: *num_locals,
                inferred_signals: *inferred_signals,
                param_bounds: param_bounds.clone(),
                doc: doc.clone(),
                origin: *origin,
                assert_numeric: *assert_numeric,
            },

            // ── Call: func and args wrap ──
            HirKind::Call {
                func,
                args,
                is_tail,
            } => HirKind::Call {
                func: Box::new(self.w(func)),
                args: args
                    .iter()
                    .map(|a| CallArg {
                        expr: self.w(&a.expr),
                        spliced: a.spliced,
                    })
                    .collect(),
                is_tail: *is_tail,
            },

            // ── Intrinsic: args wrap ──
            HirKind::Intrinsic { op, args } => HirKind::Intrinsic {
                op: *op,
                args: args.iter().map(|a| self.w(a)).collect(),
            },

            // ── If: cond/then/else wrap ──
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => HirKind::If {
                cond: Box::new(self.w(cond)),
                then_branch: Box::new(self.w(then_branch)),
                else_branch: Box::new(self.w(else_branch)),
            },

            // ── Cond: both cond and body of each clause wrap ──
            HirKind::Cond {
                clauses,
                else_branch,
            } => HirKind::Cond {
                clauses: clauses
                    .iter()
                    .map(|(c, b)| (self.w(c), self.w(b)))
                    .collect(),
                else_branch: else_branch.as_ref().map(|eb| Box::new(self.w(eb))),
            },

            // ── Begin: every expression wraps. Non-last positions
            // discard the value; the binding's slot is what
            // `emit_decrefs_for` uses to release the call result
            // region (the retired shadow stash slot is gone). ──
            HirKind::Begin(exprs) => HirKind::Begin(exprs.iter().map(|e| self.w(e)).collect()),

            // ── Block: every expression wraps; same rationale. ──
            HirKind::Block {
                name,
                block_id,
                body,
            } => HirKind::Block {
                name: name.clone(),
                block_id: *block_id,
                body: body.iter().map(|e| self.w(e)).collect(),
            },

            // ── Break.value: wrap ──
            HirKind::Break { block_id, value } => HirKind::Break {
                block_id: *block_id,
                value: Box::new(self.w(value)),
            },

            // ── Emit.value: wrap ──
            HirKind::Emit { signal, value } => HirKind::Emit {
                signal: *signal,
                value: Box::new(self.w(value)),
            },

            // ── Return.value: defensive (Return is inserted after ANF) ──
            HirKind::Return { value } => HirKind::Return {
                value: Box::new(self.w(value)),
            },

            // ── Recur.args: wrap ──
            HirKind::Recur { args } => HirKind::Recur {
                args: args.iter().map(|a| self.w(a)).collect(),
            },

            // ── Eval: both expr and env wrap ──
            HirKind::Eval { expr, env } => HirKind::Eval {
                expr: Box::new(self.w(expr)),
                env: Box::new(self.w(env)),
            },

            // ── Parameterize: bindings wrap; body propagates ──
            HirKind::Parameterize { bindings, body } => HirKind::Parameterize {
                bindings: bindings
                    .iter()
                    .map(|(k, v)| (self.w(k), self.w(v)))
                    .collect(),
                body: Box::new(self.t(body)),
            },

            // ── And/Or: each element wraps (any could be the form's value) ──
            HirKind::And(exprs) => HirKind::And(exprs.iter().map(|e| self.w(e)).collect()),
            HirKind::Or(exprs) => HirKind::Or(exprs.iter().map(|e| self.w(e)).collect()),

            // ── Match: value wraps; arm bodies and guards wrap ──
            HirKind::Match { value, arms } => HirKind::Match {
                value: Box::new(self.w(value)),
                arms: arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        (pat.clone(), guard.as_ref().map(|g| self.w(g)), self.w(body))
                    })
                    .collect(),
            },

            // ── Assign.value: wrap ──
            HirKind::Assign { target, value } => HirKind::Assign {
                target: *target,
                value: Box::new(self.w(value)),
            },

            // ── Destructure.value: wrap ──
            HirKind::Destructure {
                pattern,
                value,
                strict,
            } => HirKind::Destructure {
                pattern: pattern.clone(),
                value: Box::new(self.w(value)),
                strict: *strict,
            },

            // ── While.{cond, body}: wrap ──
            HirKind::While { cond, body } => HirKind::While {
                cond: Box::new(self.w(cond)),
                body: Box::new(self.w(body)),
            },

            // ── MakeCell/DerefCell: transparent in lowerer; don't wrap ──
            HirKind::MakeCell { value } => HirKind::MakeCell {
                value: Box::new(self.t(value)),
            },
            HirKind::DerefCell { cell } => HirKind::DerefCell {
                cell: Box::new(self.t(cell)),
            },

            // ── SetCell: cell and value wrap ──
            HirKind::SetCell { cell, value } => HirKind::SetCell {
                cell: Box::new(self.w(cell)),
                value: Box::new(self.w(value)),
            },
        };

        Hir {
            kind: new_kind,
            span: hir.span,
            signal: hir.signal,
            id: hir.id,
        }
    }
}

/// True iff `hir` is the canonical ANF wrap shape `(let [b e] (var b))`.
fn is_anf_wrapped(hir: &Hir) -> bool {
    if let HirKind::Let { bindings, body } = &hir.kind {
        if bindings.len() == 1 {
            let (b, _) = &bindings[0];
            if let HirKind::Var(bv) = &body.kind {
                return bv == b;
            }
        }
    }
    false
}

/// Short label for `[trace:anf]` output.
fn kind_label(k: &HirKind) -> &'static str {
    match k {
        HirKind::Call { .. } => "call",
        HirKind::Lambda { .. } => "lambda",
        HirKind::Eval { .. } => "eval",
        HirKind::Intrinsic { .. } => "intrinsic",
        HirKind::Match { .. } => "match",
        _ => "expr",
    }
}

// ── Tests ────────────────────────────────────────────────────────────
//
// Tests examine the HIR structure after running `anf_lift` to verify
// it conforms to the ANF discipline: every allocating expression in
// a consumer value position is wrapped in a synthetic Let.

#[cfg(test)]
mod tests;
