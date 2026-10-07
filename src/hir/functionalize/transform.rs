// audited: 2026-10-06
//! The functionalize transform's main walk: one arm per `HirKind`, delegating `begin`, `while` and branches.
//!
//! docs/impl/hir.md

use super::*;

impl<'a> FnCtx<'a> {
    /// The main transform.
    pub(super) fn transform(&mut self, hir: &Hir) -> Hir {
        let span = hir.span;
        let signal = hir.signal;

        match &hir.kind {
            HirKind::Var(b) => {
                let resolved = self.resolve(*b);
                let var_node = Hir::new(HirKind::Var(resolved), span, signal);
                if self.tracked.contains(&hir.id) {
                    self.moved.entry(hir.id).or_default().push(var_node.id);
                }
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
                            cell: Box::new(Hir::new(HirKind::Var(resolved_target), span, signal)),
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
            HirKind::Lambda(lambda) => {
                let saved_renames = self.renames.clone();
                let saved_cells = self.cell_bindings.clone();
                // Mark captured bindings that need cells
                for cap in &lambda.captures {
                    if self.arena.get(cap.binding).needs_capture() {
                        self.cell_bindings.insert(cap.binding);
                    }
                }
                // Mark mutated parameters as cell bindings
                for p in lambda.params.iter().chain(lambda.rest_param.iter()) {
                    if self.arena.get(*p).needs_capture() {
                        self.cell_bindings.insert(*p);
                    }
                }
                let new_body = self.transform(&lambda.body);
                self.renames = saved_renames;
                self.cell_bindings = saved_cells;
                Hir::new(
                    HirKind::Lambda(Box::new(lambda.with_body(new_body))),
                    span,
                    signal,
                )
            }

            // The branch forms: an SSA rename made inside an arm must not
            // escape to code the arm does not dominate (branches.rs).
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.transform_if(cond, then_branch, else_branch, span, signal),

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

            // `Return` is inserted after functionalize runs, so this is
            // defensive; transform transparently if ever present.
            HirKind::Return { value } => {
                let new_value = self.transform(value);
                Hir::new(
                    HirKind::Return {
                        value: Box::new(new_value),
                    },
                    span,
                    signal,
                )
            }

            HirKind::And(exprs) => {
                let new = self.transform_short_circuit(exprs);
                Hir::new(HirKind::And(new), span, signal)
            }

            HirKind::Or(exprs) => {
                let new = self.transform_short_circuit(exprs);
                Hir::new(HirKind::Or(new), span, signal)
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => self.transform_cond(clauses, else_branch.as_deref(), span, signal),

            HirKind::Match { value, arms } => self.transform_match(value, arms, span, signal),

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

            // This transform produces DerefCell and SetCell, and no pass
            // produces MakeCell, so none appears in the input HIR. Handle them
            // structurally anyway.
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
            | HirKind::QuoteConst(_)
            | HirKind::Error => hir.clone(),
        }
    }
}
