//! SSA conversion: eliminate Assign, convert While to Loop/Recur.
//!
//! Transforms imperative HIR (with While/Assign) into functional HIR
//! (with Loop/Recur and let-chains). This is the foundation for region
//! inference, type inference, and signal inference.
//!
//! The transform handles two patterns:
//!
//! 1. **While + Assign → Loop/Recur:** mutable bindings assigned in a
//!    while body become loop parameters; assigns become recur arguments.
//!
//! 2. **Sequential Assign in Begin → Define of fresh SSA binding:**
//!    `(assign x val)` in a begin sequence becomes `(define x' val)`,
//!    renaming subsequent uses of x to x'.
//!
//! **CaptureCell boundary:** bindings that are both captured AND mutated
//! (`needs_capture()`) stay as Assign — they're shared mutable state
//! across closure boundaries that can't be SSA-converted.
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
/// Eliminates Assign (except CaptureCell and in-branch) and converts
/// While to Loop/Recur. Modifies the arena to create fresh bindings
/// for SSA versions.
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
            HirKind::Var(b) => Hir::new(HirKind::Var(self.resolve(*b)), span, signal),

            // Standalone Assign outside Begin: leave as Assign but
            // apply renaming to target. The Begin handler converts
            // sequential assigns; standalone ones pass through.
            HirKind::Assign { target, value } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::Assign {
                        target: self.resolve(*target),
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
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

    /// Transform a Begin sequence, converting sequential Assigns to
    /// Define of fresh SSA bindings.
    fn transform_begin(&mut self, exprs: &[Hir], span: crate::syntax::Span, signal: Signal) -> Hir {
        if exprs.is_empty() {
            return Hir::new(HirKind::Nil, span, signal);
        }

        let mut result_exprs = Vec::new();

        for expr in exprs {
            match &expr.kind {
                HirKind::Assign { target, value } if !self.arena.get(*target).needs_capture() => {
                    // Sequential assign → Define of fresh SSA binding.
                    // This preserves sequential semantics: the fresh
                    // binding is allocated and stored, and subsequent
                    // expressions see the new version via renames.
                    //
                    // TODO: proper let-chain wrapping (each Define wraps
                    // the continuation) would give region inference better
                    // structural information. For now, Define (slot-based)
                    // is correct and the lowerer handles it.
                    let new_value = self.transform(value);
                    let fresh = self.fresh_version(*target);
                    self.renames.insert(*target, fresh);
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
        HirKind::Assign { value, .. } | HirKind::Define { value, .. } => f(value),
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
