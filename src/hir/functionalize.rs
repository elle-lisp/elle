//! SSA conversion: eliminate Assign, convert While to Loop/Recur,
//! explicit cell ops for CaptureCell bindings.
//!
//! Transforms imperative HIR (with While/Assign) into functional HIR
//! (with Loop/Recur, let-chains, and explicit cell operations). This is
//! the foundation for region inference, type inference, and signal inference.
//!
//! The transform handles three patterns:
//!
//! 1. **While + Assign → Loop/Recur:** mutable bindings assigned in a
//!    while body become loop parameters; assigns become recur arguments.
//!
//! 2. **Sequential Assign in Begin → Define of fresh SSA binding:**
//!    `(assign x val)` in a begin sequence becomes `(define x' val)`,
//!    renaming subsequent uses of x to x'.
//!
//! 3. **CaptureCell bindings → explicit cell ops:** bindings that
//!    `needs_capture()` get explicit DerefCell (for reads) and SetCell
//!    (for writes) in the HIR. The binding itself holds a cell; mutation
//!    goes through set-cell!, reading through deref-cell.
//!
//! **Branch boundary:** Assigns inside if/match/cond arms are left as
//! Assign. Proper phi insertion for conditional mutation requires
//! continuation context; deferred to region inference (Layer 2).

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirKind};
use crate::signals::Signal;
use std::collections::{BTreeSet, HashMap};

/// Run the functionalize transform on a HIR tree.
///
/// Eliminates Assign (except in-branch) and converts While to
/// Loop/Recur. CaptureCell bindings get explicit DerefCell/SetCell
/// ops. Modifies the arena to create fresh bindings for SSA versions.
pub fn functionalize(hir: &mut Hir, arena: &mut BindingArena) {
    let mut ctx = FnCtx {
        arena,
        renames: HashMap::new(),
        cell_bindings: BTreeSet::new(),
        assign_preserved: BTreeSet::new(),
    };
    *hir = ctx.transform(hir);
}

struct FnCtx<'a> {
    arena: &'a mut BindingArena,
    renames: HashMap<Binding, Binding>,
    /// Bindings that have been wrapped in cells (needs_capture).
    /// References to these must go through DerefCell, assigns through SetCell.
    cell_bindings: BTreeSet<Binding>,
    /// Bindings whose assigns must NOT be SSA-converted. Includes loop
    /// parameters (threaded via Recur) and outer-scope variables assigned
    /// inside a loop body (maintained via slot mutation by the lowerer).
    assign_preserved: BTreeSet<Binding>,
}

impl<'a> FnCtx<'a> {
    /// Create a fresh SSA version of a binding, copying its metadata.
    fn fresh_version(&mut self, original: Binding) -> Binding {
        let info = self.arena.get(original);
        let name = info.name;
        let scope = info.scope;
        let new_binding = self.arena.alloc(name, scope);
        self.arena.get_mut(new_binding).is_immutable = true;
        new_binding
    }

    /// Create a fresh synthetic binding with no connection to any real
    /// source binding. Used for phi-insertion condition temporaries.
    fn gensym(&mut self) -> Binding {
        let binding = self.arena.gensym();
        self.arena.get_mut(binding).is_immutable = true;
        binding
    }

    /// Look up the current SSA version of a binding, following chains.
    fn resolve(&self, b: Binding) -> Binding {
        let mut current = b;
        while let Some(&next) = self.renames.get(&current) {
            current = next;
        }
        current
    }

    /// Collect bindings that are Assign'd within a HIR subtree,
    /// excluding CaptureCell and cell_bindings. Uses BTreeSet for
    /// deterministic ordering (reproducible Loop binding order across runs).
    fn collect_assigned_bindings(&self, hir: &Hir, out: &mut BTreeSet<Binding>) {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                if !self.arena.get(*target).needs_capture() && !self.cell_bindings.contains(target)
                {
                    out.insert(*target);
                }
                self.collect_assigned_bindings(value, out);
            }
            // Don't look inside lambdas — they have their own scope
            HirKind::Lambda { .. } => {}
            _ => {
                hir.for_each_child(|child| {
                    self.collect_assigned_bindings(child, out);
                });
            }
        }
    }

    /// The main transform.
    fn transform(&mut self, hir: &Hir) -> Hir {
        let span = hir.span.clone();
        let signal = hir.signal;

        match &hir.kind {
            HirKind::Var(b) => {
                let resolved = self.resolve(*b);
                let var_node = Hir::new(HirKind::Var(resolved), span.clone(), signal);
                if self.cell_bindings.contains(&resolved) {
                    Hir::new(
                        HirKind::DerefCell {
                            cell: Box::new(var_node),
                        },
                        span,
                        signal,
                    )
                } else {
                    var_node
                }
            }

            // Standalone Assign outside Begin: CaptureCell assigns become
            // SetCell; non-capture assigns pass through (for Begin handler).
            HirKind::Assign { target, value } => {
                let resolved_target = self.resolve(*target);
                let new_value = self.transform(value);
                if self.cell_bindings.contains(&resolved_target) {
                    Hir::new(
                        HirKind::SetCell {
                            cell: Box::new(Hir::new(
                                HirKind::Var(resolved_target),
                                span.clone(),
                                signal,
                            )),
                            value: Box::new(new_value),
                        },
                        span,
                        signal,
                    )
                } else {
                    Hir::new(
                        HirKind::Assign {
                            target: resolved_target,
                            value: Box::new(new_value),
                        },
                        span,
                        signal,
                    )
                }
            }

            HirKind::While { cond, body } => {
                self.transform_while(cond, body, span, signal, &BTreeSet::new())
            }

            HirKind::Begin(exprs) => self.transform_begin(exprs, span, signal),

            // Lambda: transform body in a fresh renaming scope
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
            } => {
                let saved_renames = self.renames.clone();
                let saved_cells = self.cell_bindings.clone();
                // Mark captured bindings that need cells
                for cap in captures {
                    if self.arena.get(cap.binding).needs_capture() {
                        self.cell_bindings.insert(cap.binding);
                    }
                }
                // Mark mutated parameters as cell bindings
                for p in params.iter().chain(rest_param.iter()) {
                    if self.arena.get(*p).needs_capture() {
                        self.cell_bindings.insert(*p);
                    }
                }
                let new_body = self.transform(body);
                self.renames = saved_renames;
                self.cell_bindings = saved_cells;
                Hir::new(
                    HirKind::Lambda {
                        params: params.clone(),
                        num_required: *num_required,
                        rest_param: *rest_param,
                        vararg_kind: vararg_kind.clone(),
                        captures: captures.clone(),
                        body: Box::new(new_body),
                        num_locals: *num_locals,
                        inferred_signals: *inferred_signals,
                        param_bounds: param_bounds.clone(),
                        doc: *doc,
                        syntax: syntax.clone(),
                        assert_numeric: *assert_numeric,
                    },
                    span,
                    signal,
                )
            }

            // If: transform branches without save/restore. SSA renames
            // from assigns in branches propagate outward. When the If
            // is directly in a begin sequence, transform_begin_at handles
            // phi-insertion for proper merge semantics. When nested
            // (e.g. inside let body), assigns in branches are either:
            // - cell-backed (letrec mutated bindings) → set-cell is correct
            // - in a begin context that handles phi-insertion
            // - simple cases where propagation is harmless
            //
            // Cond/Match DO save/restore because they have multiple
            // alternative branches; If has exactly two branches and the
            // begin-level phi handles the merge.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let new_cond = self.transform(cond);
                let new_then = self.transform(then_branch);
                let new_else = self.transform(else_branch);
                Hir::new(
                    HirKind::If {
                        cond: Box::new(new_cond),
                        then_branch: Box::new(new_then),
                        else_branch: Box::new(new_else),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Let { bindings, body } => {
                let new_bindings: Vec<_> = bindings
                    .iter()
                    .map(|(b, init)| {
                        let new_init = self.transform(init);
                        if self.arena.get(*b).needs_capture() {
                            self.cell_bindings.insert(*b);
                        }
                        (*b, new_init)
                    })
                    .collect();
                let new_body = self.transform(body);
                Hir::new(
                    HirKind::Let {
                        bindings: new_bindings,
                        body: Box::new(new_body),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Letrec { bindings, body } => {
                // Pre-register cell bindings so that forward references
                // within the letrec body see them as cell-wrapped.
                // Letrec inits are NOT wrapped in MakeCell — the lowerer
                // handles two-pass cell init (create cell in pass 1, store
                // value into existing cell in pass 2) so that forward
                // references through closures see the shared cell.
                //
                // Also mark mutated bindings as cell-backed even when not
                // captured. This ensures assigns in branches (if/match/cond)
                // go through SetCell rather than SSA conversion, which is
                // necessary because SSA renames from one branch must not
                // leak to subsequent letrec bindings.
                for (b, _) in bindings {
                    let bi = self.arena.get(*b);
                    if bi.needs_capture() || bi.is_mutated {
                        self.cell_bindings.insert(*b);
                    }
                }
                let new_bindings: Vec<_> = bindings
                    .iter()
                    .map(|(b, init)| (*b, self.transform(init)))
                    .collect();
                let new_body = self.transform(body);
                Hir::new(
                    HirKind::Letrec {
                        bindings: new_bindings,
                        body: Box::new(new_body),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                let new_func = self.transform(func);
                let new_args: Vec<_> = args
                    .iter()
                    .map(|a| CallArg {
                        expr: self.transform(&a.expr),
                        spliced: a.spliced,
                    })
                    .collect();
                Hir::new(
                    HirKind::Call {
                        func: Box::new(new_func),
                        args: new_args,
                        is_tail: *is_tail,
                    },
                    span,
                    signal,
                )
            }

            HirKind::Define { binding, value } => {
                let new_value = self.transform(value);
                // Define appears in Begin sequences with pre-allocation
                // (two-pass: pass 1 creates cell, pass 2 stores value).
                // Don't wrap in MakeCell — the lowerer handles cell init.
                if self.arena.get(*binding).needs_capture() {
                    self.cell_bindings.insert(*binding);
                }
                Hir::new(
                    HirKind::Define {
                        binding: *binding,
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Block {
                name,
                block_id,
                body,
            } => {
                let new_body: Vec<_> = body.iter().map(|e| self.transform(e)).collect();
                Hir::new(
                    HirKind::Block {
                        name: name.clone(),
                        block_id: *block_id,
                        body: new_body,
                    },
                    span,
                    signal,
                )
            }

            HirKind::Break { block_id, value } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::Break {
                        block_id: *block_id,
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Emit { signal: sig, value } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::Emit {
                        signal: *sig,
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }

            HirKind::And(exprs) => {
                let new: Vec<_> = exprs.iter().map(|e| self.transform(e)).collect();
                Hir::new(HirKind::And(new), span, signal)
            }

            HirKind::Or(exprs) => {
                let new: Vec<_> = exprs.iter().map(|e| self.transform(e)).collect();
                Hir::new(HirKind::Or(new), span, signal)
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let saved = self.renames.clone();
                let new_clauses: Vec<_> = clauses
                    .iter()
                    .map(|(c, b)| {
                        self.renames = saved.clone();
                        (self.transform(c), self.transform(b))
                    })
                    .collect();
                self.renames = saved.clone();
                let new_else = else_branch.as_ref().map(|e| Box::new(self.transform(e)));
                self.renames = saved;
                Hir::new(
                    HirKind::Cond {
                        clauses: new_clauses,
                        else_branch: new_else,
                    },
                    span,
                    signal,
                )
            }

            HirKind::Match { value, arms } => {
                let new_value = self.transform(value);
                let saved = self.renames.clone();
                let new_arms: Vec<_> = arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        self.renames = saved.clone();
                        (
                            pat.clone(),
                            guard.as_ref().map(|g| self.transform(g)),
                            self.transform(body),
                        )
                    })
                    .collect();
                self.renames = saved;
                Hir::new(
                    HirKind::Match {
                        value: Box::new(new_value),
                        arms: new_arms,
                    },
                    span,
                    signal,
                )
            }

            HirKind::Destructure {
                pattern,
                value,
                strict,
            } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::Destructure {
                        pattern: pattern.clone(),
                        value: Box::new(new_value),
                        strict: *strict,
                    },
                    span,
                    signal,
                )
            }

            HirKind::Eval { expr, env } => {
                let new_expr = self.transform(expr);
                let new_env = self.transform(env);
                Hir::new(
                    HirKind::Eval {
                        expr: Box::new(new_expr),
                        env: Box::new(new_env),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Parameterize { bindings, body } => {
                let new_bindings: Vec<_> = bindings
                    .iter()
                    .map(|(k, v)| (self.transform(k), self.transform(v)))
                    .collect();
                let new_body = self.transform(body);
                Hir::new(
                    HirKind::Parameterize {
                        bindings: new_bindings,
                        body: Box::new(new_body),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Loop { bindings, body } => {
                let new_bindings: Vec<_> = bindings
                    .iter()
                    .map(|(b, init)| (*b, self.transform(init)))
                    .collect();
                let new_body = self.transform(body);
                Hir::new(
                    HirKind::Loop {
                        bindings: new_bindings,
                        body: Box::new(new_body),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Recur { args } => {
                let new_args: Vec<_> = args.iter().map(|a| self.transform(a)).collect();
                Hir::new(HirKind::Recur { args: new_args }, span, signal)
            }

            // Cell ops are produced by this transform; they should not
            // appear in the input HIR. Handle them structurally for safety.
            HirKind::MakeCell { value } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::MakeCell {
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }
            HirKind::DerefCell { cell } => {
                let new_cell = self.transform(cell);
                Hir::new(
                    HirKind::DerefCell {
                        cell: Box::new(new_cell),
                    },
                    span,
                    signal,
                )
            }
            HirKind::SetCell { cell, value } => {
                let new_cell = self.transform(cell);
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::SetCell {
                        cell: Box::new(new_cell),
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }

            HirKind::Intrinsic { op, args } => {
                let new_args: Vec<_> = args.iter().map(|a| self.transform(a)).collect();
                Hir::new(
                    HirKind::Intrinsic {
                        op: *op,
                        args: new_args,
                    },
                    span,
                    signal,
                )
            }

            // Leaves: no children to transform
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::Error => hir.clone(),
        }
    }

    /// Collect bindings introduced (Define or Let/Letrec) within a HIR
    /// subtree. Used to filter out locally-scoped bindings from while→loop
    /// parameter promotion.
    fn collect_locally_introduced(hir: &Hir, out: &mut BTreeSet<Binding>) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                out.insert(*binding);
                Self::collect_locally_introduced(value, out);
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    out.insert(*b);
                    Self::collect_locally_introduced(init, out);
                }
                Self::collect_locally_introduced(body, out);
            }
            HirKind::Lambda { .. } => {} // Don't look inside lambdas
            _ => {
                hir.for_each_child(|child| {
                    Self::collect_locally_introduced(child, out);
                });
            }
        }
    }

    /// Transform a While loop into a Loop/Recur.
    ///
    /// `scope_defines`: bindings Define'd in the while's enclosing begin
    /// (sibling defines). Only these bindings (plus those defined inside
    /// the while body) can be promoted to loop parameters. Bindings from
    /// outer scopes stay as Assign — the lowerer handles them via slot
    /// mutation.
    fn transform_while(
        &mut self,
        cond: &Hir,
        body: &Hir,
        span: crate::syntax::Span,
        signal: Signal,
        scope_defines: &BTreeSet<Binding>,
    ) -> Hir {
        // Collect bindings assigned in the loop body (deterministic order)
        let mut assigned = BTreeSet::new();
        self.collect_assigned_bindings(body, &mut assigned);
        self.collect_assigned_bindings(cond, &mut assigned);

        // Filter out bindings introduced inside the while body (via
        // Define, Let, or Letrec) — they can't be loop parameters since
        // they don't exist before the loop starts.
        let mut locally_introduced = BTreeSet::new();
        Self::collect_locally_introduced(body, &mut locally_introduced);
        Self::collect_locally_introduced(cond, &mut locally_introduced);
        assigned.retain(|b| !locally_introduced.contains(b));

        // Only promote bindings that are in the while's enclosing scope
        // (sibling defines). Outer-scope bindings stay as Assign — their
        // values are maintained via slot mutation by the lowerer.
        if !scope_defines.is_empty() {
            assigned.retain(|b| scope_defines.contains(b));
        }

        // Create fresh bindings for loop parameters
        let loop_bindings: Vec<(Binding, Binding)> = assigned
            .iter()
            .map(|&orig| {
                let fresh = self.fresh_version(orig);
                (orig, fresh)
            })
            .collect();

        // Initial values: the current version of each binding
        let init_bindings: Vec<(Binding, Hir)> = loop_bindings
            .iter()
            .map(|&(orig, fresh)| {
                let current = self.resolve(orig);
                (fresh, Hir::silent(HirKind::Var(current), span.clone()))
            })
            .collect();

        // Inside the loop, rename original bindings to fresh versions.
        // Also mark ALL bindings assigned in the body (including outer
        // variables) as assign_preserved to prevent SSA conversion — they
        // must stay as Assign for runtime slot mutation.
        let saved = self.renames.clone();
        let saved_assign_preserved = self.assign_preserved.clone();
        for &(orig, fresh) in &loop_bindings {
            self.renames.insert(orig, fresh);
            self.assign_preserved.insert(fresh);
        }
        // Collect ALL assigned bindings (before any filtering) and mark
        // their resolved versions as assign_preserved too, so outer variables
        // assigned inside the loop body aren't SSA-converted.
        let mut all_body_assigned = BTreeSet::new();
        self.collect_assigned_bindings(body, &mut all_body_assigned);
        self.collect_assigned_bindings(cond, &mut all_body_assigned);
        for b in &all_body_assigned {
            let resolved = self.resolve(*b);
            self.assign_preserved.insert(resolved);
        }

        // Transform condition and body with new names
        let new_cond = self.transform(cond);
        let transformed_body = self.transform(body);

        // Append Recur with current values of loop bindings
        let recur_args: Vec<Hir> = loop_bindings
            .iter()
            .map(|&(orig, _)| {
                let current = self.resolve(orig);
                Hir::silent(HirKind::Var(current), span.clone())
            })
            .collect();
        let recur_node = Hir::silent(HirKind::Recur { args: recur_args }, span.clone());
        let body_with_recur = Hir::new(
            HirKind::Begin(vec![transformed_body, recur_node]),
            span.clone(),
            body.signal,
        );

        // Restore renames and assign_preserved, then set loop parameter
        // versions as active (code after the loop sees them)
        self.renames = saved;
        self.assign_preserved = saved_assign_preserved;
        for &(orig, fresh) in &loop_bindings {
            self.renames.insert(orig, fresh);
        }

        // Build: (loop [bindings...] (if cond (begin body recur) nil))
        Hir::new(
            HirKind::Loop {
                bindings: init_bindings,
                body: Box::new(Hir::new(
                    HirKind::If {
                        cond: Box::new(new_cond),
                        then_branch: Box::new(body_with_recur),
                        else_branch: Box::new(Hir::silent(HirKind::Nil, span.clone())),
                    },
                    span.clone(),
                    signal,
                )),
            },
            span,
            signal,
        )
    }

    /// Transform a Begin sequence. Processes expressions left-to-right:
    /// - Assign → Let wrapping the continuation (proper SSA let-chain)
    /// - If/Cond/Match containing assigns → phi-let insertion after merge
    /// - Everything else → transform and continue
    fn transform_begin(&mut self, exprs: &[Hir], span: crate::syntax::Span, signal: Signal) -> Hir {
        self.transform_begin_at(exprs, 0, span, signal)
    }

    /// Recursive helper: transform exprs[start..] as a begin sequence.
    fn transform_begin_at(
        &mut self,
        exprs: &[Hir],
        start: usize,
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        if start >= exprs.len() {
            return Hir::new(HirKind::Nil, span, signal);
        }

        let expr = &exprs[start];

        // Sequential assign → Let wrapping the continuation.
        // Skip SSA for loop parameters (threaded via Recur) and cell bindings.
        if let HirKind::Assign { target, value } = &expr.kind {
            let resolved_target = self.resolve(*target);
            if !self.arena.get(resolved_target).needs_capture()
                && !self.cell_bindings.contains(&resolved_target)
                && !self.assign_preserved.contains(&resolved_target)
            {
                let new_value = self.transform(value);
                let fresh = self.fresh_version(resolved_target);
                self.renames.insert(resolved_target, fresh);
                let continuation = self.transform_begin_at(exprs, start + 1, span.clone(), signal);
                return Hir::new(
                    HirKind::Let {
                        bindings: vec![(fresh, new_value)],
                        body: Box::new(continuation),
                    },
                    span,
                    signal,
                );
            }
        }

        // If with assigns in branches → transform + phi-let insertion
        if let HirKind::If {
            cond,
            then_branch,
            else_branch,
        } = &expr.kind
        {
            let mut then_assigns = BTreeSet::new();
            let mut else_assigns = BTreeSet::new();
            self.collect_assigned_bindings(then_branch, &mut then_assigns);
            self.collect_assigned_bindings(else_branch, &mut else_assigns);
            let all_assigned: BTreeSet<_> = then_assigns
                .union(&else_assigns)
                .copied()
                .filter(|b| !self.assign_preserved.contains(&self.resolve(*b)))
                .collect();

            if !all_assigned.is_empty() {
                return self.transform_if_with_phi(
                    cond,
                    then_branch,
                    else_branch,
                    &all_assigned,
                    exprs,
                    start,
                    span,
                    signal,
                );
            }
        }

        // While in a begin (possibly wrapped in a Block): collect
        // sibling defines for scope context. The analyzer wraps `while`
        // in a Block for break support, so we unwrap it here.
        let while_parts = match &expr.kind {
            HirKind::While { cond, body } => Some((cond.as_ref(), body.as_ref())),
            HirKind::Block {
                body: block_body, ..
            } if block_body.len() == 1 && matches!(block_body[0].kind, HirKind::While { .. }) => {
                if let HirKind::While { cond, body } = &block_body[0].kind {
                    Some((cond.as_ref(), body.as_ref()))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((cond, body)) = while_parts {
            // Collect Define bindings from earlier expressions in this begin
            let mut scope_defines = BTreeSet::new();
            for prior in &exprs[..start] {
                if let HirKind::Define { binding, .. } = &prior.kind {
                    scope_defines.insert(*binding);
                }
            }
            let mut transformed =
                self.transform_while(cond, body, expr.span.clone(), expr.signal, &scope_defines);
            // Re-wrap in Block if the original While was Block-wrapped
            if let HirKind::Block { name, block_id, .. } = &expr.kind {
                transformed = Hir::new(
                    HirKind::Block {
                        name: name.clone(),
                        block_id: *block_id,
                        body: vec![transformed],
                    },
                    expr.span.clone(),
                    expr.signal,
                );
            }
            if start + 1 >= exprs.len() {
                return transformed;
            }
            let rest = self.transform_begin_at(exprs, start + 1, span.clone(), signal);
            return Hir::new(HirKind::Begin(vec![transformed, rest]), span, signal);
        }

        // Default: transform this expr, then the rest
        let transformed = self.transform(expr);
        if start + 1 >= exprs.len() {
            // Last expression — its value is the Begin's result
            return transformed;
        }
        let rest = self.transform_begin_at(exprs, start + 1, span.clone(), signal);
        Hir::new(HirKind::Begin(vec![transformed, rest]), span, signal)
    }

    /// Transform an If that contains assigns in its branches, inserting
    /// phi-lets after the merge point for each assigned binding.
    ///
    /// ```text
    /// (begin (if cond (assign x 1)) (println x))
    /// →
    /// (let [x_1 (if cond 1 x_0)]
    ///   (println x_1))
    /// ```
    #[allow(clippy::too_many_arguments)]
    fn transform_if_with_phi(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
        assigned: &BTreeSet<Binding>,
        exprs: &[Hir],
        start: usize,
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        let new_cond = self.transform(cond);

        // Bind the condition to a temporary so that phi-selects don't
        // re-evaluate it (the condition may reference mutable cells that
        // the then-branch modifies).
        let cond_binding = self.gensym();
        let cond_var = Hir::silent(HirKind::Var(cond_binding), cond.span.clone());

        // Transform each branch, extracting the final SSA value of
        // each assigned binding. Assigns are removed from the branch
        // body; their values are collected for phi construction.
        let saved = self.renames.clone();

        let (new_then, then_versions) =
            self.transform_branch_extracting_assigns(then_branch, assigned);
        self.renames = saved.clone();

        let (new_else, else_versions) =
            self.transform_branch_extracting_assigns(else_branch, assigned);
        self.renames = saved.clone();

        // Emit the If (with assigns removed from branches)
        let if_expr = Hir::new(
            HirKind::If {
                cond: Box::new(cond_var.clone()),
                then_branch: Box::new(new_then),
                else_branch: Box::new(new_else),
            },
            cond.span.clone(),
            signal,
        );

        // Build phi-lets: for each assigned binding, create
        // (let [x_fresh (if cond_var then_val else_val)] ...continuation...)
        // Using cond_var (not new_cond) ensures the phi tests the same
        // value as the if, even when branches modify the condition's inputs.
        let phi_bindings: Vec<_> = assigned
            .iter()
            .map(|&orig| {
                let then_val = then_versions
                    .get(&orig)
                    .map(|&b| Hir::silent(HirKind::Var(b), span.clone()))
                    .unwrap_or_else(|| {
                        // Not assigned in then → use pre-if version
                        let pre = saved.get(&orig).copied().unwrap_or(orig);
                        Hir::silent(HirKind::Var(pre), span.clone())
                    });
                let else_val = else_versions
                    .get(&orig)
                    .map(|&b| Hir::silent(HirKind::Var(b), span.clone()))
                    .unwrap_or_else(|| {
                        let pre = saved.get(&orig).copied().unwrap_or(orig);
                        Hir::silent(HirKind::Var(pre), span.clone())
                    });

                let fresh = self.fresh_version(orig);
                self.renames.insert(orig, fresh);

                let phi_val = Hir::new(
                    HirKind::If {
                        cond: Box::new(cond_var.clone()),
                        then_branch: Box::new(then_val),
                        else_branch: Box::new(else_val),
                    },
                    span.clone(),
                    Signal::silent(),
                );
                (fresh, phi_val)
            })
            .collect();

        // Transform the continuation with the phi bindings active
        let has_continuation = start + 1 < exprs.len();
        let mut result = self.transform_begin_at(exprs, start + 1, span.clone(), signal);

        // Wrap: if_expr; (let [phis...] continuation)
        for (binding, phi_val) in phi_bindings.into_iter().rev() {
            result = Hir::new(
                HirKind::Let {
                    bindings: vec![(binding, phi_val)],
                    body: Box::new(result),
                },
                span.clone(),
                signal,
            );
        }

        if has_continuation {
            // Wrap in let for the condition binding, then prepend the if
            let inner = Hir::new(HirKind::Begin(vec![if_expr, result]), span.clone(), signal);
            Hir::new(
                HirKind::Let {
                    bindings: vec![(cond_binding, new_cond)],
                    body: Box::new(inner),
                },
                span,
                signal,
            )
        } else {
            // The if is the last expression in the begin. The phi-lets
            // wrap a nil continuation, so (begin if_expr phi_lets) would
            // evaluate to nil. Capture the if's value in a temp, nest the
            // phi-lets inside the temp's body, and return the temp.
            let result_binding = self.gensym();
            let result_var = Hir::silent(HirKind::Var(result_binding), span.clone());
            // (let [cond_binding new_cond]
            //   (let [result_binding if_expr]
            //     (let [phi1 ...]
            //       (let [phi2 ...]
            //         result_var))))
            Hir::new(
                HirKind::Let {
                    bindings: vec![(cond_binding, new_cond)],
                    body: Box::new(Hir::new(
                        HirKind::Let {
                            bindings: vec![(result_binding, if_expr)],
                            body: Box::new(Hir::new(
                                HirKind::Begin(vec![result, result_var]),
                                span.clone(),
                                signal,
                            )),
                        },
                        span.clone(),
                        signal,
                    )),
                },
                span,
                signal,
            )
        }
    }

    /// Transform a branch body, converting assigns to the target
    /// bindings into Defines (so the value is captured) and recording
    /// which SSA version each binding ended up at.
    fn transform_branch_extracting_assigns(
        &mut self,
        branch: &Hir,
        targets: &BTreeSet<Binding>,
    ) -> (Hir, HashMap<Binding, Binding>) {
        // Transform the branch normally — assigns inside it become
        // Defines (via transform_begin) or stay as Assign.
        let transformed = self.transform(branch);
        // Collect the final SSA version of each target binding
        let versions: HashMap<Binding, Binding> = targets
            .iter()
            .filter_map(|&orig| {
                let current = self.resolve(orig);
                if current != orig {
                    Some((orig, current))
                } else {
                    None
                }
            })
            .collect();
        (transformed, versions)
    }
}

#[cfg(test)]
mod tests {
    use crate::context::{set_symbol_table, set_vm_context};
    use crate::pipeline::eval_all;
    use crate::primitives::register_primitives;
    use crate::symbol::SymbolTable;
    use crate::value::Value;
    use crate::vm::VM;

    fn eval_bare(source: &str) -> Result<Value, String> {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        eval_all(source, &mut symbols, &mut vm, "<test>")
    }

    fn eval_with_stdlib(source: &str) -> Result<Value, String> {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        set_vm_context(&mut vm as *mut VM);
        set_symbol_table(&mut symbols as *mut SymbolTable);
        crate::init_stdlib(&mut vm, &mut symbols);
        let result = eval_all(source, &mut symbols, &mut vm, "<test>");
        set_vm_context(std::ptr::null_mut());
        result
    }

    #[test]
    fn if_result_preserved_with_phi_merge_last_in_begin() {
        // When an `if` containing assigns is the last expression in a begin,
        // the phi-lets must not discard the if's result value.
        let result = eval_bare(
            r#"(do
                    (var x 0)
                    (if true
                        (do (assign x 1) "yes")
                        (do (assign x 2) "no")))"#,
        )
        .unwrap();
        assert_eq!(result, Value::string("yes"));
    }

    #[test]
    fn if_result_preserved_with_phi_merge_else_branch() {
        // Same as above but the else branch is taken.
        let result = eval_bare(
            r#"(do
                    (var x 0)
                    (if false
                        (do (assign x 1) "yes")
                        (do (assign x 2) "no")))"#,
        )
        .unwrap();
        assert_eq!(result, Value::string("no"));
    }

    #[test]
    fn if_result_preserved_with_each_loop() {
        // The original bug: `each` expands to a match with mutable defines
        // inside branches, triggering phi insertion that discards the
        // if's return value.
        let result = eval_with_stdlib(
            r#"(do
                    (defn f [x]
                      (let [[a b] ["." x]]
                        (if (= b "")
                          @[]
                          (let [acc @[]]
                            (each i in (list 1 2 3) (push acc i))
                            acc))))
                    (f "hello"))"#,
        )
        .unwrap();
        // @[] creates a mutable array — use as_array_mut
        let arr = result.as_array_mut().expect("expected mutable array");
        let arr = arr.borrow();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], Value::int(1));
        assert_eq!(arr[1], Value::int(2));
        assert_eq!(arr[2], Value::int(3));
    }

    #[test]
    fn if_phi_merge_with_continuation_still_works() {
        // When the if is NOT the last expression, the phi-lets should
        // still correctly merge the assigned value for downstream use.
        let result = eval_bare(
            r#"(do
                    (var x 0)
                    (if true
                        (assign x 42)
                        (assign x 99))
                    x)"#,
        )
        .unwrap();
        assert_eq!(result, Value::int(42));
    }
}
