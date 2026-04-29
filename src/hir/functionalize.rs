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
    };
    *hir = ctx.transform(hir);
}

struct FnCtx<'a> {
    arena: &'a mut BindingArena,
    renames: HashMap<Binding, Binding>,
    /// Bindings that have been wrapped in cells (needs_capture).
    /// References to these must go through DerefCell, assigns through SetCell.
    cell_bindings: BTreeSet<Binding>,
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

    /// Look up the current SSA version of a binding.
    fn resolve(&self, b: Binding) -> Binding {
        self.renames.get(&b).copied().unwrap_or(b)
    }

    /// Collect bindings that are Assign'd within a HIR subtree,
    /// excluding CaptureCell bindings. Uses BTreeSet for deterministic
    /// ordering (reproducible Loop binding order across runs).
    fn collect_assigned_bindings(&self, hir: &Hir, out: &mut BTreeSet<Binding>) {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                if !self.arena.get(*target).needs_capture() {
                    out.insert(*target);
                }
                self.collect_assigned_bindings(value, out);
            }
            // Don't look inside lambdas — they have their own scope
            HirKind::Lambda { .. } => {}
            _ => {
                for_each_child(hir, |child| {
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

            HirKind::While { cond, body } => self.transform_while(cond, body, span, signal),

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

            // No forking/merging for branches — assigns inside if/match/cond
            // stay as Assign. Phi insertion deferred to Layer 2.
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
                for (b, _) in bindings {
                    if self.arena.get(*b).needs_capture() {
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
                let new_clauses: Vec<_> = clauses
                    .iter()
                    .map(|(c, b)| (self.transform(c), self.transform(b)))
                    .collect();
                let new_else = else_branch.as_ref().map(|e| Box::new(self.transform(e)));
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
                let new_arms: Vec<_> = arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        (
                            pat.clone(),
                            guard.as_ref().map(|g| self.transform(g)),
                            self.transform(body),
                        )
                    })
                    .collect();
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

            _ => hir.clone(),
        }
    }

    /// Transform a While loop into a Loop/Recur.
    fn transform_while(
        &mut self,
        cond: &Hir,
        body: &Hir,
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        // Collect bindings assigned in the loop body (deterministic order)
        let mut assigned = BTreeSet::new();
        self.collect_assigned_bindings(body, &mut assigned);
        self.collect_assigned_bindings(cond, &mut assigned);

        if assigned.is_empty() {
            // No mutations — leave as While
            let new_cond = self.transform(cond);
            let new_body = self.transform(body);
            return Hir::new(
                HirKind::While {
                    cond: Box::new(new_cond),
                    body: Box::new(new_body),
                },
                span,
                signal,
            );
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

        // Inside the loop, rename original bindings to fresh versions
        let saved = self.renames.clone();
        for &(orig, fresh) in &loop_bindings {
            self.renames.insert(orig, fresh);
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

        // Restore renames, then set loop parameter versions as active
        // (code after the loop sees the loop parameter bindings)
        self.renames = saved;
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

        // Sequential assign → Let wrapping the continuation
        if let HirKind::Assign { target, value } = &expr.kind {
            if !self.arena.get(*target).needs_capture() {
                let new_value = self.transform(value);
                let fresh = self.fresh_version(*target);
                self.renames.insert(*target, fresh);
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
            let all_assigned: BTreeSet<_> = then_assigns.union(&else_assigns).copied().collect();

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
                cond: Box::new(new_cond.clone()),
                then_branch: Box::new(new_then),
                else_branch: Box::new(new_else),
            },
            cond.span.clone(),
            signal,
        );

        // Build phi-lets: for each assigned binding, create
        // (let [x_fresh (if cond then_val else_val)] ...continuation...)
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
                        cond: Box::new(new_cond.clone()),
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

        // Prepend the if expression (for its side effects)
        Hir::new(HirKind::Begin(vec![if_expr, result]), span, signal)
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

/// Iterate over the immediate child HIR nodes of a node.
fn for_each_child(hir: &Hir, mut f: impl FnMut(&Hir)) {
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

        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (_, init) in bindings {
                f(init);
            }
            f(body);
        }
        HirKind::Lambda { body, .. } => f(body),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            f(cond);
            f(then_branch);
            f(else_branch);
        }
        HirKind::Begin(exprs) => {
            for e in exprs {
                f(e);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                f(e);
            }
        }
        HirKind::Break { value, .. } => f(value),
        HirKind::Call { func, args, .. } => {
            f(func);
            for a in args {
                f(&a.expr);
            }
        }
        HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::MakeCell { value } => f(value),
        HirKind::DerefCell { cell } => f(cell),
        HirKind::SetCell { cell, value } => {
            f(cell);
            f(value);
        }
        HirKind::While { cond, body } => {
            f(cond);
            f(body);
        }
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                f(init);
            }
            f(body);
        }
        HirKind::Recur { args } => {
            for a in args {
                f(a);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                f(e);
            }
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                f(c);
                f(b);
            }
            if let Some(eb) = else_branch {
                f(eb);
            }
        }
        HirKind::Emit { value, .. } => f(value),
        HirKind::Match { value, arms } => {
            f(value);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    f(g);
                }
                f(body);
            }
        }
        HirKind::Destructure { value, .. } => f(value),
        HirKind::Eval { expr, env } => {
            f(expr);
            f(env);
        }
        HirKind::Parameterize { bindings, body } => {
            for (_, v) in bindings {
                f(v);
            }
            f(body);
        }
    }
}
