use super::*;

impl<'a> FnCtx<'a> {
    /// Collect bindings introduced (Define or Let/Letrec) within a HIR
    /// subtree. Used to filter out locally-scoped bindings from while→loop
    /// parameter promotion.
    pub(super) fn collect_locally_introduced(hir: &Hir, out: &mut BTreeSet<Binding>) {
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
    pub(super) fn transform_while(
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
    pub(super) fn transform_begin(
        &mut self,
        exprs: &[Hir],
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        self.transform_begin_at(exprs, 0, span, signal)
    }
    /// Recursive helper: transform exprs[start..] as a begin sequence.
    pub(super) fn transform_begin_at(
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

        // Cond with assigns in branches → phi-insertion
        if let HirKind::Cond {
            clauses,
            else_branch,
        } = &expr.kind
        {
            let mut all_assigned = BTreeSet::new();
            for (_, body) in clauses {
                self.collect_assigned_bindings(body, &mut all_assigned);
            }
            if let Some(e) = else_branch {
                self.collect_assigned_bindings(e, &mut all_assigned);
            }
            all_assigned.retain(|b| !self.assign_preserved.contains(&self.resolve(*b)));
            if !all_assigned.is_empty() {
                return self.transform_cond_with_phi(
                    clauses,
                    else_branch,
                    &all_assigned,
                    exprs,
                    start,
                    span,
                    signal,
                );
            }
        }

        // Match with assigns in arms → phi-insertion
        if let HirKind::Match { value, arms } = &expr.kind {
            let mut all_assigned = BTreeSet::new();
            for (_, _, body) in arms {
                self.collect_assigned_bindings(body, &mut all_assigned);
            }
            all_assigned.retain(|b| !self.assign_preserved.contains(&self.resolve(*b)));
            if !all_assigned.is_empty() {
                return self.transform_match_with_phi(
                    value,
                    arms,
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
    pub(super) fn transform_if_with_phi(
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
    pub(super) fn transform_branch_extracting_assigns(
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
