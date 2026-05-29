//! A-normal form (ANF) lift.
//!
//! Names every allocating expression by wrapping it in a synthetic
//! `let` whose body is the bound variable. After this pass, every
//! heap-allocating value has a `Binding` — meaning the lowerer can
//! key slot ownership entirely off `binding_to_slot`, with no shadow
//! mechanism for un-named call results.
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
//! **Already named (do NOT wrap):**
//! `Let` / `Letrec` / `Loop` binding RHS; `Define.value`.
//!
//! **Transparent in the lowerer (do NOT wrap — Finding 1):**
//! `MakeCell.value`, `DerefCell.cell`. The lowerer is transparent for
//! these and the implicit `MakeCaptureCell` happens at the binding
//! site; wrapping their child manufactures a region with no matching
//! allocation.
//!
//! **Propagating tail positions (do NOT wrap — the outer consumer
//! wraps the form itself):**
//! `Let` / `Letrec` / `Loop` body; `Lambda.body`;
//! `Parameterize.body`; `Begin` non-last; `Block` non-last.
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
/// When `--anf=off` is set on the CLI, this is a no-op — the
/// counter-factual switch used by `tests/integration/anf_counterfactual.rs`
/// to demonstrate that the transform is causally responsible for
/// fixing the closure-binding-overwrite bug class (Family C). The
/// switch should be removed in a follow-up once causality is
/// reviewed.
pub fn anf_lift(hir: &mut Hir, arena: &mut BindingArena) {
    if !crate::config::get().anf {
        return;
    }
    let mut ctx = AnfCtx { arena };
    *hir = ctx.transform(hir);
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
    fn name_if_alloc(&mut self, hir: Hir) -> Hir {
        if is_anf_wrapped(&hir) {
            return hir;
        }
        if !hir.allocates() {
            return hir;
        }
        let span = hir.span.clone();
        let signal = hir.signal;
        let b = self.gensym();
        let var = Hir::silent(HirKind::Var(b), span.clone());
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

    /// Transform a child in a NON-WRAP position: recurse into its
    /// own children but do not wrap the resulting node at this level.
    /// Used for binding-RHS positions, MakeCell/DerefCell pass-through
    /// children, and propagating tail bodies.
    fn t(&mut self, hir: &Hir) -> Hir {
        self.transform(hir)
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
            | HirKind::Error => hir.kind.clone(),

            // ── Binding forms: RHS is already named ──
            HirKind::Let { bindings, body } => HirKind::Let {
                bindings: bindings
                    .iter()
                    .map(|(b, init)| (*b, self.t(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },
            HirKind::Letrec { bindings, body } => HirKind::Letrec {
                bindings: bindings
                    .iter()
                    .map(|(b, init)| (*b, self.t(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },
            HirKind::Loop { bindings, body } => HirKind::Loop {
                bindings: bindings
                    .iter()
                    .map(|(b, init)| (*b, self.t(init)))
                    .collect(),
                body: Box::new(self.t(body)),
            },

            // ── Define: value position is already named ──
            HirKind::Define { binding, value } => HirKind::Define {
                binding: *binding,
                value: Box::new(self.t(value)),
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
                syntax,
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
                doc: *doc,
                syntax: syntax.clone(),
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

            // ── MakeCell/DerefCell: transparent in lowerer; don't wrap (Finding 1) ──
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
            span: hir.span.clone(),
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
//
// These tests are written against the *intended* behavior, not the
// current no-op skeleton — they will fail until `anf_lift` is
// implemented. That's deliberate: the failing test is the
// counter-factual proof that the test catches the bug we're fixing.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::analyze::Analyzer;
    use crate::hir::expr::{HirId, HirKind};
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::BindingArena;
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    fn analyze_anf(source: &str) -> (Hir, BindingArena, SymbolTable) {
        // Wrap source so `f`, `g`, `h`, `cond_var` are bound as fns and
        // available inside the test expression. The letrec puts them in
        // an outer scope; the analyzer doesn't inline letrec-bound
        // closures, so calls to them survive into HIR as real Calls.
        let wrapped = format!(
            "(letrec [cond_var (fn () true) \
                      f (fn (& args) args) \
                      g (fn (& args) args) \
                      h (fn (& args) args)] {})",
            source
        );
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);

        let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand failed");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);
        anf_lift(&mut analysis.hir, &mut arena);
        (analysis.hir, arena, symbols)
    }

    /// Pre-order walk that hands out borrowed references with the
    /// SAME lifetime as the input — unlike `Hir::for_each_child`,
    /// whose `FnMut(&Hir)` is higher-rank and can't let the borrow
    /// escape. We pay the verbosity tax to get correct lifetimes.
    fn walk_pre<'a, F: FnMut(&'a Hir)>(hir: &'a Hir, f: &mut F) {
        f(hir);
        match &hir.kind {
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Var(_)
            | HirKind::Quote(_)
            | HirKind::Error => {}
            HirKind::Let { bindings, body }
            | HirKind::Letrec { bindings, body }
            | HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    walk_pre(init, f);
                }
                walk_pre(body, f);
            }
            HirKind::Lambda { body, .. } => walk_pre(body, f),
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                walk_pre(cond, f);
                walk_pre(then_branch, f);
                walk_pre(else_branch, f);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    walk_pre(c, f);
                    walk_pre(b, f);
                }
                if let Some(eb) = else_branch {
                    walk_pre(eb, f);
                }
            }
            HirKind::Begin(es) => {
                for e in es {
                    walk_pre(e, f);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    walk_pre(e, f);
                }
            }
            HirKind::Break { value, .. } => walk_pre(value, f),
            HirKind::Call { func, args, .. } => {
                walk_pre(func, f);
                for a in args {
                    walk_pre(&a.expr, f);
                }
            }
            HirKind::Assign { value, .. }
            | HirKind::Define { value, .. }
            | HirKind::MakeCell { value } => walk_pre(value, f),
            HirKind::DerefCell { cell } => walk_pre(cell, f),
            HirKind::SetCell { cell, value } => {
                walk_pre(cell, f);
                walk_pre(value, f);
            }
            HirKind::While { cond, body } => {
                walk_pre(cond, f);
                walk_pre(body, f);
            }
            HirKind::Recur { args } => {
                for a in args {
                    walk_pre(a, f);
                }
            }
            HirKind::And(es) | HirKind::Or(es) => {
                for e in es {
                    walk_pre(e, f);
                }
            }
            HirKind::Emit { value, .. } => walk_pre(value, f),
            HirKind::Match { value, arms } => {
                walk_pre(value, f);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        walk_pre(g, f);
                    }
                    walk_pre(body, f);
                }
            }
            HirKind::Destructure { value, .. } => walk_pre(value, f),
            HirKind::Eval { expr, env } => {
                walk_pre(expr, f);
                walk_pre(env, f);
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    walk_pre(k, f);
                    walk_pre(v, f);
                }
                walk_pre(body, f);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    walk_pre(a, f);
                }
            }
        }
    }

    /// Strip `DerefCell` wrappers introduced by `functionalize` for
    /// needs-capture bindings. `Var(g)` becomes `DerefCell(Var(g))`
    /// when `g` is captured by a nested closure; tests address the
    /// inner `Var`.
    fn unwrap_deref_cell(hir: &Hir) -> &Hir {
        if let HirKind::DerefCell { cell } = &hir.kind {
            cell
        } else {
            hir
        }
    }

    /// Find every Call whose `func` is `Var(name)` (resolved via
    /// symbols), looking through `DerefCell`.
    fn find_calls_to<'a>(
        hir: &'a Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
    ) -> Vec<&'a Hir> {
        let mut out = Vec::new();
        let mut visit = |node: &'a Hir| {
            if let HirKind::Call { func, .. } = &node.kind {
                let func_ref = unwrap_deref_cell(func);
                if let HirKind::Var(b) = &func_ref.kind {
                    if symbols.name(arena.get(*b).name) == Some(name) {
                        out.push(node);
                    }
                }
            }
        };
        walk_pre(hir, &mut visit);
        out
    }

    /// HirId-based lookup helper.
    #[allow(dead_code)]
    fn find_node<'a>(hir: &'a Hir, target: HirId) -> Option<&'a Hir> {
        let mut found: Option<&'a Hir> = None;
        let mut visit = |node: &'a Hir| {
            if found.is_none() && node.id == target {
                found = Some(node);
            }
        };
        walk_pre(hir, &mut visit);
        found
    }

    /// True if `hir` is `(let [b e] (var b))` — the ANF wrap shape.
    fn is_anf_wrap(hir: &Hir) -> bool {
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

    /// Extract the init expression from an ANF wrap. Panics if not a wrap.
    fn anf_wrap_init(hir: &Hir) -> &Hir {
        match &hir.kind {
            HirKind::Let { bindings, .. } if bindings.len() == 1 => &bindings[0].1,
            _ => panic!("expected ANF wrap"),
        }
    }

    // ── 1. (g (f x)): outer Call's arg lifted ────────────────────

    #[test]
    fn inline_call_arg_is_lifted() {
        let (hir, arena, symbols) = analyze_anf("(g (f 1))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1, "expected one call to g");
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(
            is_anf_wrap(arg0),
            "expected (g (let [t (f 1)] t)) — got {:?}",
            arg0.kind
        );
        // And the wrapped init must be the original Call to f.
        let init = anf_wrap_init(arg0);
        match &init.kind {
            HirKind::Call { func, .. } => match &func.kind {
                HirKind::Var(b) => assert_eq!(symbols.name(arena.get(*b).name), Some("f")),
                _ => panic!("expected Call to f"),
            },
            _ => panic!("expected init to be a Call"),
        }
    }

    // ── 2. (let [a (f x)] a): init not re-lifted ──────────────────

    #[test]
    fn let_init_call_is_not_relifted() {
        let (hir, _arena, _symbols) = analyze_anf("(let [a (f 1)] a)");
        // Find every Let with a single binding whose init is a direct Call.
        // The user's outer Let — `(let [a (f 1)] a)` — qualifies and must
        // remain. Re-wrapping its init would produce
        // `(let [a (let [t (f 1)] t)] a)`: the init is already a name,
        // re-wrap is redundant and bloats the IR. We assert *some* such
        // Let exists in the post-ANF HIR.
        let mut found_direct_call_init = false;
        let mut visit = |node: &Hir| {
            if let HirKind::Let { bindings, .. } = &node.kind {
                if bindings.len() == 1 && matches!(&bindings[0].1.kind, HirKind::Call { .. }) {
                    found_direct_call_init = true;
                }
            }
        };
        walk_pre(&hir, &mut visit);
        assert!(
            found_direct_call_init,
            "the user Let `(let [a (f 1)] a)` must keep its direct-Call init \
             (no re-wrap into `(let [a (let [t (f 1)] t)] a)`)"
        );
    }

    // ── 3. (g (%pair 1 2)): allocating intrinsic in Call arg lifted ──

    #[test]
    fn pair_intrinsic_in_call_arg_is_lifted() {
        let (hir, arena, symbols) = analyze_anf("(g (%pair 1 2))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1);
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(
            is_anf_wrap(arg0),
            "expected (g (let [t (%pair 1 2)] t)) — got {:?}",
            arg0.kind
        );
        let init = anf_wrap_init(arg0);
        assert!(
            matches!(&init.kind, HirKind::Intrinsic { .. }),
            "expected init to be the %pair intrinsic"
        );
    }

    // ── 4. (h (g (f x))): nested calls chain into Lets ───────────

    #[test]
    fn nested_calls_chain_into_lets() {
        let (hir, arena, symbols) = analyze_anf("(h (g (f 1)))");
        let h_calls = find_calls_to(&hir, "h", &arena, &symbols);
        assert_eq!(h_calls.len(), 1);
        let h_call = h_calls[0];
        // h's arg: Let wrap around (g ...).
        let arg_to_h = match &h_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(is_anf_wrap(arg_to_h), "h's arg must be ANF wrap");
        let inner_g_call = anf_wrap_init(arg_to_h);
        match &inner_g_call.kind {
            HirKind::Call { func, args, .. } => {
                match &func.kind {
                    HirKind::Var(b) => {
                        assert_eq!(symbols.name(arena.get(*b).name), Some("g"))
                    }
                    _ => panic!("expected g"),
                }
                // g's arg must itself be an ANF wrap around (f 1).
                assert!(is_anf_wrap(&args[0].expr), "g's arg must be ANF wrap");
                let f_call = anf_wrap_init(&args[0].expr);
                if let HirKind::Call { func: ff, .. } = &f_call.kind {
                    if let HirKind::Var(b) = &ff.kind {
                        assert_eq!(symbols.name(arena.get(*b).name), Some("f"));
                    } else {
                        panic!("expected Call to f");
                    }
                }
            }
            _ => panic!("expected g Call"),
        }
    }

    // ── 5. (g (fn () 1)): Lambda in Call arg is lifted ───────────

    #[test]
    fn lambda_as_arg_is_lifted() {
        let (hir, arena, symbols) = analyze_anf("(g (fn () 1))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1);
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(is_anf_wrap(arg0), "Lambda arg must be ANF wrap");
        let init = anf_wrap_init(arg0);
        assert!(
            matches!(&init.kind, HirKind::Lambda { .. }),
            "expected Lambda as the wrapped init — got {:?}",
            init.kind
        );
    }

    // ── 6. (g (%add 1 2)): non-allocating intrinsic NOT lifted ───

    #[test]
    fn non_allocating_intrinsic_is_not_lifted() {
        let (hir, arena, symbols) = analyze_anf("(g (%add 1 2))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1);
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(
            !is_anf_wrap(arg0),
            "non-allocating intrinsic must NOT be wrapped — got {:?}",
            arg0.kind
        );
        assert!(
            matches!(&arg0.kind, HirKind::Intrinsic { .. }),
            "expected raw Intrinsic"
        );
    }

    // ── 7. (g (if c (f) (h))): branch results lifted in branch positions ──

    #[test]
    fn if_branch_result_lifted_when_used_as_value() {
        let (hir, arena, symbols) = analyze_anf("(g (if true (f) (h)))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1);
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        // The If itself is not allocating, so it shouldn't be wrapped at
        // Call.args[0]. (See `if_does_not_allocate_itself` in expr.rs.)
        let if_node = arg0;
        let (then_b, else_b) = match &if_node.kind {
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => (then_branch.as_ref(), else_branch.as_ref()),
            _ => panic!("expected If node — got {:?}", arg0.kind),
        };
        assert!(
            is_anf_wrap(then_b),
            "then branch (f) must be ANF wrap — got {:?}",
            then_b.kind
        );
        assert!(
            is_anf_wrap(else_b),
            "else branch (h) must be ANF wrap — got {:?}",
            else_b.kind
        );
    }

    // ── 8. tail call in let body keeps is_tail marker ────────────

    #[test]
    fn tail_call_in_let_body_keeps_is_tail_marker() {
        // `(fn () (let [a 1] (g a)))` — (g a) is the body of the let,
        // which is the tail of the lambda. mark_tail_calls runs before
        // anf_lift; the is_tail flag on (g a) must survive.
        //
        // If ANF wraps the tail Call (as `(let [t (g a)] t)`), the
        // wrapped Call still has is_tail=true even though it's no longer
        // syntactically in tail position — body_is_tail_call recognizes
        // the wrap shape as tail-equivalent.
        let (hir, arena, symbols) = analyze_anf("((fn () (let [a 1] (g a))))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert!(!g_calls.is_empty(), "expected at least one call to g");
        for call_node in &g_calls {
            if let HirKind::Call { is_tail, .. } = &call_node.kind {
                assert!(
                    *is_tail,
                    "Call(g) was tail before ANF; must remain tail after"
                );
            }
        }
    }

    // ── 9. match value with rest pattern is lifted ────────────────

    #[test]
    fn match_with_rest_pattern_is_lifted() {
        // `(g (match v (@[& rest] rest)))` — the match value position
        // (Call.args[0]) is wrapped because Match allocates iff any arm
        // pattern allocates; @[& rest] (Array with rest) does. The
        // wrap lets the lowerer associate the match result's region
        // with the synthetic binding's slot.
        let (hir, arena, symbols) =
            analyze_anf("(let [v (f 1 2 3)] (g (match v @[& rest] rest _ nil)))");
        let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
        assert_eq!(g_calls.len(), 1);
        let g_call = g_calls[0];
        let arg0 = match &g_call.kind {
            HirKind::Call { args, .. } => &args[0].expr,
            _ => unreachable!(),
        };
        assert!(
            is_anf_wrap(arg0),
            "Match with allocating rest pattern must be ANF wrap"
        );
        let init = anf_wrap_init(arg0);
        assert!(
            matches!(&init.kind, HirKind::Match { .. }),
            "expected Match as wrapped init — got {:?}",
            init.kind
        );
    }
}
