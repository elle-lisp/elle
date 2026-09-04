use super::*;

impl<'a> FnCtx<'a> {
    /// Transform a Cond that contains assigns in its branches, inserting
    /// phi-lets after the merge point.
    ///
    /// Strategy: bind each condition to a temporary (with short-circuit
    /// evaluation so later conditions aren't evaluated if an earlier one
    /// was true), then use those temps for both the cond and the
    /// phi-selects (nested Ifs over the temps).
    ///
    /// ```text
    /// (begin (cond test1 (assign x 1) test2 (assign x 2) (assign x 3)) (use x))
    /// →
    /// (let [t1 test1]
    ///   (let [t2 (if t1 false test2)]
    ///     (begin
    ///       (cond t1 body1' t2 body2' else')
    ///       (let [x_phi (if t1 x_v1 (if t2 x_v2 x_v3))]
    ///         (use x_phi)))))
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub(super) fn transform_cond_with_phi(
        &mut self,
        clauses: &[(Hir, Hir)],
        else_branch: &Option<Box<Hir>>,
        assigned: &BTreeSet<Binding>,
        exprs: &[Hir],
        start: usize,
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        let saved = self.renames.clone();

        // 1. Transform each condition and branch body, extracting assign versions
        let mut cond_exprs = Vec::new();
        let mut bodies = Vec::new();
        let mut all_versions: Vec<HashMap<Binding, Binding>> = Vec::new();

        for (cond_expr, body) in clauses {
            self.renames = saved.clone();
            let new_cond = self.transform(cond_expr);
            cond_exprs.push(new_cond);

            self.renames = saved.clone();
            let (new_body, versions) = self.transform_branch_extracting_assigns(body, assigned);
            bodies.push(new_body);
            all_versions.push(versions);
        }

        let new_else = if let Some(e) = else_branch {
            self.renames = saved.clone();
            let (new_e, versions) = self.transform_branch_extracting_assigns(e, assigned);
            all_versions.push(versions);
            Some(new_e)
        } else {
            None
        };
        self.renames = saved.clone();

        // 2. Create condition temporaries
        let cond_temps: Vec<Binding> = (0..clauses.len()).map(|_| self.gensym()).collect();

        // 3. Build the cond using temps as conditions
        let new_clauses: Vec<(Hir, Hir)> = cond_temps
            .iter()
            .zip(bodies)
            .map(|(&t, body)| (Hir::silent(HirKind::Var(t), span), body))
            .collect();
        let cond_node = Hir::new(
            HirKind::Cond {
                clauses: new_clauses,
                else_branch: new_else.map(Box::new),
            },
            span,
            signal,
        );

        // 4. Build phi-selects for each assigned binding
        //    (if t1 x_v1 (if t2 x_v2 ... x_velse))
        let phi_bindings: Vec<_> = assigned
            .iter()
            .map(|&orig| {
                // Build the nested-if phi-select from right to left
                let pre = saved.get(&orig).copied().unwrap_or(orig);

                // Start with the else/default value
                let else_idx = clauses.len(); // else is after all clauses
                let mut phi_val = if else_idx < all_versions.len() {
                    // There's an else branch
                    all_versions[else_idx]
                        .get(&orig)
                        .map(|&b| Hir::silent(HirKind::Var(b), span))
                        .unwrap_or_else(|| Hir::silent(HirKind::Var(pre), span))
                } else {
                    // No else → use pre-cond version
                    Hir::silent(HirKind::Var(pre), span)
                };

                // Wrap in (if tN x_vN ...) from last clause to first
                for i in (0..clauses.len()).rev() {
                    let clause_val = all_versions[i]
                        .get(&orig)
                        .map(|&b| Hir::silent(HirKind::Var(b), span))
                        .unwrap_or_else(|| Hir::silent(HirKind::Var(pre), span));

                    phi_val = Hir::new(
                        HirKind::If {
                            cond: Box::new(Hir::silent(HirKind::Var(cond_temps[i]), span)),
                            then_branch: Box::new(clause_val),
                            else_branch: Box::new(phi_val),
                        },
                        span,
                        Signal::silent(),
                    );
                }

                let fresh = self.fresh_version(orig);
                self.renames.insert(orig, fresh);
                (fresh, phi_val)
            })
            .collect();

        // 5. Build continuation
        let has_continuation = start + 1 < exprs.len();
        let mut result = self.transform_begin_at(exprs, start + 1, span, signal);

        // Wrap: (let [phi1 ...] (let [phi2 ...] continuation))
        for (binding, phi_val) in phi_bindings.into_iter().rev() {
            result = Hir::new(
                HirKind::Let {
                    bindings: vec![(binding, phi_val)],
                    body: Box::new(result),
                },
                span,
                signal,
            );
        }

        // 6. Build condition temp lets with short-circuit evaluation
        //    t1 = test1
        //    t2 = (if t1 false test2)
        //    t3 = (if t1 false (if t2 false test3))
        //    ...
        let inner = if has_continuation {
            Hir::new(HirKind::Begin(vec![cond_node, result]), span, signal)
        } else {
            // Last expression: capture cond result in a temp
            let result_binding = self.gensym();
            let result_var = Hir::silent(HirKind::Var(result_binding), span);
            Hir::new(
                HirKind::Let {
                    bindings: vec![(result_binding, cond_node)],
                    body: Box::new(Hir::new(
                        HirKind::Begin(vec![result, result_var]),
                        span,
                        signal,
                    )),
                },
                span,
                signal,
            )
        };

        // Wrap with condition temp lets (innermost = last condition)
        let mut wrapped = inner;
        for i in (0..clauses.len()).rev() {
            let cond_init = if i == 0 {
                // First condition: evaluate directly
                cond_exprs[i].clone()
            } else {
                // Later conditions: short-circuit if any earlier test was true
                //   (if t_{i-1} false (if t_{i-2} false ... test_i))
                // Simplified: (if (or t1 ... t_{i-1}) false test_i)
                // But we can build nested ifs:
                //   (if t1 false (if t2 false ... test_i))
                let mut short_circuit = cond_exprs[i].clone();
                for j in (0..i).rev() {
                    short_circuit = Hir::new(
                        HirKind::If {
                            cond: Box::new(Hir::silent(HirKind::Var(cond_temps[j]), span)),
                            then_branch: Box::new(Hir::silent(HirKind::Bool(false), span)),
                            else_branch: Box::new(short_circuit),
                        },
                        span,
                        Signal::silent(),
                    );
                }
                short_circuit
            };

            wrapped = Hir::new(
                HirKind::Let {
                    bindings: vec![(cond_temps[i], cond_init)],
                    body: Box::new(wrapped),
                },
                span,
                signal,
            );
        }

        wrapped
    }
    /// Transform a Match that contains assigns in its arms, inserting
    /// phi-lets after the merge point.
    ///
    /// Strategy: bind the match value to a temp, use the match for the
    /// body, then build a second match for each phi-select.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn transform_match_with_phi(
        &mut self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        assigned: &BTreeSet<Binding>,
        exprs: &[Hir],
        start: usize,
        span: crate::syntax::Span,
        signal: Signal,
    ) -> Hir {
        let saved = self.renames.clone();

        // 1. Transform the match value
        let new_value = self.transform(value);
        let value_binding = self.gensym();
        let value_var = Hir::silent(HirKind::Var(value_binding), span);

        // 2. Transform each arm body, extracting assign versions
        let mut new_arms = Vec::new();
        let mut all_versions: Vec<HashMap<Binding, Binding>> = Vec::new();

        for (pat, guard, body) in arms {
            self.renames = saved.clone();
            let new_guard = guard.as_ref().map(|g| self.transform(g));
            let (new_body, versions) = self.transform_branch_extracting_assigns(body, assigned);
            new_arms.push((pat.clone(), new_guard, new_body));
            all_versions.push(versions);
        }
        self.renames = saved.clone();

        // 3. Build the match using the value temp
        let match_node = Hir::new(
            HirKind::Match {
                value: Box::new(value_var.clone()),
                arms: new_arms,
            },
            span,
            signal,
        );

        // 4. Build phi-selects: a second match for each assigned binding
        let phi_bindings: Vec<_> = assigned
            .iter()
            .map(|&orig| {
                let pre = saved.get(&orig).copied().unwrap_or(orig);

                // Build match arms for the phi-select
                let mut phi_arms: Vec<_> = arms
                    .iter()
                    .zip(all_versions.iter())
                    .map(|((pat, guard, _), versions)| {
                        let phi_val = versions
                            .get(&orig)
                            .map(|&b| Hir::silent(HirKind::Var(b), span))
                            .unwrap_or_else(|| Hir::silent(HirKind::Var(pre), span));
                        // Replicate guards for the phi-select match
                        let phi_guard = guard.as_ref().map(|g| self.transform(g));
                        (pat.clone(), phi_guard, phi_val)
                    })
                    .collect();
                // Synthetic catch-all: the phi-select re-evaluates guards,
                // and an impure guard may answer differently the second
                // time. The real match already ran (a no-match there raised
                // before this point), so fall back to the pre-match version
                // rather than raising a spurious :match-error. On guard
                // divergence this selects the stale pre-value — the same
                // behavior a user-written catch-all arm produces.
                phi_arms.push((
                    HirPattern::Wildcard,
                    None,
                    Hir::silent(HirKind::Var(pre), span),
                ));

                let phi_match = Hir::new(
                    HirKind::Match {
                        value: Box::new(value_var.clone()),
                        arms: phi_arms,
                    },
                    span,
                    Signal::silent(),
                );

                let fresh = self.fresh_version(orig);
                self.renames.insert(orig, fresh);
                (fresh, phi_match)
            })
            .collect();

        // 5. Build continuation
        let has_continuation = start + 1 < exprs.len();
        let mut result = self.transform_begin_at(exprs, start + 1, span, signal);

        // Wrap: (let [phi1 ...] continuation)
        for (binding, phi_val) in phi_bindings.into_iter().rev() {
            result = Hir::new(
                HirKind::Let {
                    bindings: vec![(binding, phi_val)],
                    body: Box::new(result),
                },
                span,
                signal,
            );
        }

        // 6. Wrap with match and value binding
        let inner = if has_continuation {
            Hir::new(HirKind::Begin(vec![match_node, result]), span, signal)
        } else {
            let result_binding = self.gensym();
            let result_var = Hir::silent(HirKind::Var(result_binding), span);
            Hir::new(
                HirKind::Let {
                    bindings: vec![(result_binding, match_node)],
                    body: Box::new(Hir::new(
                        HirKind::Begin(vec![result, result_var]),
                        span,
                        signal,
                    )),
                },
                span,
                signal,
            )
        };

        Hir::new(
            HirKind::Let {
                bindings: vec![(value_binding, new_value)],
                body: Box::new(inner),
            },
            span,
            signal,
        )
    }
}
