use super::*;

impl LivenessAnalyzer {
    /// Compute liveness for a HIR node. `live_after` is the set of bindings
    /// live after this node. Returns the set of bindings live before this node.
    pub fn analyze(&mut self, hir: &Hir, live_after: &BitSet) -> BitSet {
        self.live_out.insert(hir.id, live_after.clone());

        match &hir.kind {
            // Leaves
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_)
            | HirKind::Error => live_after.clone(),

            HirKind::Var(b) => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(b) {
                    live.set(idx);
                }
                live
            }

            HirKind::Begin(exprs) => self.analyze_sequence(exprs, live_after),

            HirKind::Block { body, .. } => self.analyze_sequence(body, live_after),

            HirKind::Let { bindings, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_body;
                // Process bindings right-to-left: init's live_out is the
                // live set needed after it (including the bound variable,
                // since it will be used in the body). Then remove the bound
                // variable to get live_in at the Let level.
                for (b, init) in bindings.iter().rev() {
                    // live currently has whatever the body/later bindings need.
                    // The init's live_out IS live (which may include b if used in body).
                    live = self.analyze(init, &live);
                    // After processing init, remove b — it's defined by this Let,
                    // so it's not live before the Let.
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                live
            }

            HirKind::Letrec { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                // Remove all bound names first (mutually recursive)
                for (b, _) in bindings {
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                // Walk inits
                for (_, init) in bindings.iter().rev() {
                    live = self.analyze(init, &live);
                }
                live
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let live_then = self.analyze(then_branch, live_after);
                let live_else = self.analyze(else_branch, live_after);
                let mut live_cond_after = live_then;
                live_cond_after.union_with(&live_else);
                self.analyze(cond, &live_cond_after)
            }

            HirKind::Lambda { captures, body, .. } => {
                // Lambda body is a separate liveness scope
                let body_live_after = self.empty_set();
                self.analyze(body, &body_live_after);

                // The lambda node generates uses for its captures
                let mut live = live_after.clone();
                for cap in captures {
                    if let Some(&idx) = self.binding_index.get(&cap.binding) {
                        live.set(idx);
                    }
                }
                live
            }

            HirKind::Call { func, args, .. } => {
                let mut live = live_after.clone();
                // Process args right-to-left
                for a in args.iter().rev() {
                    live = self.analyze(&a.expr, &live);
                }
                self.analyze(func, &live)
            }

            HirKind::Define { binding, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(binding) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Assign { target, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(target) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Loop { bindings, body } => self.analyze_loop(bindings, body, live_after),

            HirKind::Recur { args } => {
                // Recur generates uses of its args — they flow to loop bindings.
                // The actual binding happens at the loop node. Here we just
                // ensure the args are live.
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }

            HirKind::Break { value, .. } => {
                // Break exits the block — value needs to be live
                self.analyze(value, live_after)
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any expr could be the last one evaluated.
                // Conservative: union of live-in from each suffix.
                let mut live = live_after.clone();
                for e in exprs.iter().rev() {
                    let live_e = self.analyze(e, &live);
                    live.union_with(&live_e);
                    live = live_e;
                }
                live
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut live = if let Some(eb) = else_branch {
                    self.analyze(eb, live_after)
                } else {
                    live_after.clone()
                };
                for (c, b) in clauses.iter().rev() {
                    let live_body = self.analyze(b, live_after);
                    live.union_with(&live_body);
                    live = self.analyze(c, &live);
                }
                live
            }

            HirKind::Match { value, arms } => {
                let mut live_after_scrutinee = self.empty_set();
                for (pat, guard, body) in arms {
                    let mut live_arm = self.analyze(body, live_after);
                    if let Some(g) = guard {
                        live_arm = self.analyze(g, &live_arm);
                    }
                    // Remove pattern bindings
                    for b in pat.bindings().bindings {
                        if let Some(&idx) = self.binding_index.get(&b) {
                            live_arm.clear(idx);
                        }
                    }
                    live_after_scrutinee.union_with(&live_arm);
                }
                self.analyze(value, &live_after_scrutinee)
            }

            HirKind::Emit { value, .. } => self.analyze(value, live_after),

            HirKind::Return { value } => self.analyze(value, live_after),

            HirKind::MakeCell { value } => self.analyze(value, live_after),

            HirKind::DerefCell { cell } => self.analyze(cell, live_after),

            HirKind::SetCell { cell, value } => {
                let live = self.analyze(value, live_after);
                self.analyze(cell, &live)
            }

            HirKind::Destructure { pattern, value, .. } => {
                let mut live = live_after.clone();
                for b in pattern.bindings().bindings {
                    if let Some(&idx) = self.binding_index.get(&b) {
                        live.clear(idx);
                    }
                }
                self.analyze(value, &live)
            }

            HirKind::Eval { expr, env } => {
                let live = self.analyze(env, live_after);
                self.analyze(expr, &live)
            }

            HirKind::Parameterize { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                for (k, v) in bindings.iter().rev() {
                    live = self.analyze(v, &live);
                    live = self.analyze(k, &live);
                }
                live
            }

            HirKind::While { cond, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_after.clone();
                live.union_with(&live_body);
                self.analyze(cond, &live)
            }

            HirKind::Intrinsic { args, .. } => {
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }
        }
    }
}
