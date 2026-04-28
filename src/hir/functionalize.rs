//! SSA conversion: eliminate Assign, convert While to Loop/Recur.
//!
//! Transforms imperative HIR (with While/Assign) into functional HIR
//! (with Loop/Recur and let-chains). This is the foundation for region
//! inference, type inference, and signal inference.
//!
//! The transform walks the HIR with a renaming environment that maps
//! original mutable bindings to their current SSA version. Each Assign
//! creates a fresh binding version; While becomes Loop/Recur with the
//! assigned bindings as loop parameters.
//!
//! **CaptureCell boundary:** bindings that are both captured AND mutated
//! (`needs_capture()`) stay as Assign — they're shared mutable state
//! across closure boundaries that can't be SSA-converted.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirKind};
use crate::signals::Signal;
use std::collections::{HashMap, HashSet};

/// Run the functionalize transform on a HIR tree.
///
/// Eliminates Assign (except CaptureCell) and converts While to Loop/Recur.
/// Modifies the arena to create fresh bindings for SSA versions.
pub fn functionalize(hir: &mut Hir, arena: &mut BindingArena) {
    let mut ctx = FnCtx {
        arena,
        renames: HashMap::new(),
    };
    *hir = ctx.transform(hir);
}

struct FnCtx<'a> {
    arena: &'a mut BindingArena,
    renames: HashMap<Binding, Binding>,
}

impl<'a> FnCtx<'a> {
    /// Create a fresh SSA version of a binding, copying its metadata.
    fn fresh_version(&mut self, original: Binding) -> Binding {
        let info = self.arena.get(original);
        let name = info.name;
        let scope = info.scope;
        let new_binding = self.arena.alloc(name, scope);
        // The new version is immutable (SSA — single assignment)
        self.arena.get_mut(new_binding).is_immutable = true;
        new_binding
    }

    /// Look up the current SSA version of a binding.
    fn resolve(&self, b: Binding) -> Binding {
        self.renames.get(&b).copied().unwrap_or(b)
    }

    /// Collect bindings that are Assign'd within a HIR subtree,
    /// excluding CaptureCell bindings (captured + mutated).
    fn collect_assigned_bindings(&self, hir: &Hir, out: &mut HashSet<Binding>) {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                if !self.arena.get(*target).needs_capture() {
                    out.insert(*target);
                }
                self.collect_assigned_bindings(value, out);
            }
            HirKind::Lambda { .. } => {
                // Don't look inside lambdas — they have their own scope
            }
            _ => {
                self.for_each_child(hir, |child| {
                    self.collect_assigned_bindings(child, out);
                });
            }
        }
    }

    /// Iterate over the immediate child HIR nodes of a node.
    fn for_each_child(&self, hir: &Hir, mut f: impl FnMut(&Hir)) {
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
            HirKind::Assign { value, .. } => f(value),
            HirKind::Define { value, .. } => f(value),
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

    /// The main transform. Returns a new HIR tree with Assign eliminated
    /// and While converted to Loop/Recur.
    fn transform(&mut self, hir: &Hir) -> Hir {
        let span = hir.span.clone();
        let signal = hir.signal;

        match &hir.kind {
            // Var: apply renaming
            HirKind::Var(b) => Hir::new(HirKind::Var(self.resolve(*b)), span, signal),

            // Assign: convert to let-chain (handled in transform_begin for sequences)
            // Standalone assign outside begin — wrap in continuation
            HirKind::Assign { target, value } => {
                if self.arena.get(*target).needs_capture() {
                    // CaptureCell: leave as Assign, but transform the value
                    let new_value = self.transform(value);
                    Hir::new(
                        HirKind::Assign {
                            target: self.resolve(*target),
                            value: Box::new(new_value),
                        },
                        span,
                        signal,
                    )
                } else {
                    // SSA: create a new version. Since this is a standalone assign
                    // (not in a Begin), the new version shadows the old one but
                    // there's no continuation to use it — just return the value.
                    let new_value = self.transform(value);
                    let fresh = self.fresh_version(*target);
                    self.renames.insert(*target, fresh);
                    Hir::new(
                        HirKind::Let {
                            bindings: vec![(fresh, new_value)],
                            body: Box::new(Hir::silent(HirKind::Nil, span.clone())),
                        },
                        span,
                        signal,
                    )
                }
            }

            // While: convert to Loop/Recur
            HirKind::While { cond, body } => self.transform_while(cond, body, span, signal),

            // Begin: handle sequential Assigns as let-chains
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
                let saved = self.renames.clone();
                let new_body = self.transform(body);
                self.renames = saved;
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

            // Structural recursion for everything else
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let new_cond = self.transform(cond);
                // Fork renaming for branches
                let saved = self.renames.clone();
                let new_then = self.transform(then_branch);
                let then_renames = self.renames.clone();
                self.renames = saved.clone();
                let new_else = self.transform(else_branch);
                let else_renames = self.renames.clone();

                // Merge: for bindings that diverge, create phi let-bindings
                let result = Hir::new(
                    HirKind::If {
                        cond: Box::new(new_cond),
                        then_branch: Box::new(new_then),
                        else_branch: Box::new(new_else),
                    },
                    span.clone(),
                    signal,
                );
                self.merge_branches(result, &then_renames, &else_renames, &saved, span, signal)
            }

            HirKind::Let { bindings, body } => {
                let new_bindings: Vec<_> = bindings
                    .iter()
                    .map(|(b, init)| (*b, self.transform(init)))
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
                let new_exprs: Vec<_> = exprs.iter().map(|e| self.transform(e)).collect();
                Hir::new(HirKind::And(new_exprs), span, signal)
            }

            HirKind::Or(exprs) => {
                let new_exprs: Vec<_> = exprs.iter().map(|e| self.transform(e)).collect();
                Hir::new(HirKind::Or(new_exprs), span, signal)
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

            // Pass-through for leaves and already-functional forms
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

            // Leaves
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
        // Collect bindings assigned in the loop body
        let mut assigned = HashSet::new();
        self.collect_assigned_bindings(body, &mut assigned);
        // Also check condition for assigns (rare but possible)
        self.collect_assigned_bindings(cond, &mut assigned);

        if assigned.is_empty() {
            // No mutations — leave as While (infinite loop or condition-only)
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
        let new_body = self.transform_loop_body(body, &loop_bindings, &span);

        // After the loop, the bindings refer to the loop parameter versions
        // (which the lowerer updates via Recur StoreLocal)
        // Keep the renamed versions active for code after the loop
        // But we need to restore and then set the final versions
        self.renames = saved;
        for &(orig, fresh) in &loop_bindings {
            self.renames.insert(orig, fresh);
        }

        // Build: (loop [bindings...] (if cond (begin body (recur ...)) nil))
        Hir::new(
            HirKind::Loop {
                bindings: init_bindings,
                body: Box::new(Hir::new(
                    HirKind::If {
                        cond: Box::new(new_cond),
                        then_branch: Box::new(new_body),
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

    /// Transform a loop body, replacing Assigns to loop bindings with Recur.
    fn transform_loop_body(
        &mut self,
        body: &Hir,
        loop_bindings: &[(Binding, Binding)],
        span: &crate::syntax::Span,
    ) -> Hir {
        // Transform the body, then append a Recur with current values
        let transformed = self.transform(body);

        // Build recur args: current value of each loop binding
        let recur_args: Vec<Hir> = loop_bindings
            .iter()
            .map(|&(orig, _fresh)| {
                let current = self.resolve(orig);
                Hir::silent(HirKind::Var(current), span.clone())
            })
            .collect();

        let recur_node = Hir::silent(HirKind::Recur { args: recur_args }, span.clone());

        // Wrap: (begin transformed-body recur)
        Hir::new(
            HirKind::Begin(vec![transformed, recur_node]),
            span.clone(),
            body.signal,
        )
    }

    /// Transform a Begin sequence, converting Assigns to let-chains.
    fn transform_begin(&mut self, exprs: &[Hir], span: crate::syntax::Span, signal: Signal) -> Hir {
        if exprs.is_empty() {
            return Hir::new(HirKind::Nil, span, signal);
        }

        let mut result_exprs = Vec::new();

        for expr in exprs {
            match &expr.kind {
                HirKind::Assign { target, value } if !self.arena.get(*target).needs_capture() => {
                    // SSA conversion: this assign becomes a let-binding
                    // wrapping the remainder of the begin sequence.
                    // But we're iterating, so just transform the value,
                    // create a fresh version, and update the rename map.
                    // The actual let-wrapping happens at the end if needed.
                    let new_value = self.transform(value);
                    let fresh = self.fresh_version(*target);
                    self.renames.insert(*target, fresh);

                    // Emit as a let-binding around the rest
                    // We can't easily wrap the "rest" here in iteration,
                    // so emit a Define (which the lowerer handles as StoreLocal)
                    // with the fresh binding. This preserves sequential semantics.
                    result_exprs.push(Hir::new(
                        HirKind::Define {
                            binding: fresh,
                            value: Box::new(new_value),
                        },
                        expr.span.clone(),
                        expr.signal,
                    ));
                }
                _ => {
                    result_exprs.push(self.transform(expr));
                }
            }
        }

        if result_exprs.len() == 1 {
            result_exprs.pop().unwrap()
        } else {
            Hir::new(HirKind::Begin(result_exprs), span, signal)
        }
    }

    /// After an If with divergent branches, merge the renaming envs.
    /// If a binding has different versions in then vs else, we need to
    /// keep the If result accessible. For now, just pick the most recent
    /// version (this is conservative — full phi insertion can come later).
    fn merge_branches(
        &mut self,
        if_expr: Hir,
        then_renames: &HashMap<Binding, Binding>,
        else_renames: &HashMap<Binding, Binding>,
        saved: &HashMap<Binding, Binding>,
        _span: crate::syntax::Span,
        _signal: Signal,
    ) -> Hir {
        // For now: if a binding diverges, pick the else version
        // (it's the "fall-through" path). Full phi-insertion is
        // a future refinement.
        let mut merged = saved.clone();
        for (k, v) in then_renames {
            merged.insert(*k, *v);
        }
        for (k, v) in else_renames {
            merged.insert(*k, *v);
        }
        self.renames = merged;
        if_expr
    }
}
