use super::*;

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        if forms.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        // The file's top-level bindings are a macro definition environment:
        // template free variables must resolve here past use-site shadows.
        self.push_definition_scope();

        let mut entries: Vec<PreBound> = Vec::new();
        let mut gensym_counter = 0u32;
        // Track names seen in Pass 1 to detect duplicates.
        // Duplicate names are deferred to Pass 2 for sequential shadowing.
        let mut seen_names: HashSet<String> = HashSet::new();

        // Pass 1: pre-bind all names for mutual visibility.
        for form in &forms {
            match form {
                FileForm::Def(name_syntax, value_syntax)
                | FileForm::Var(name_syntax, value_syntax) => {
                    let immutable = matches!(form, FileForm::Def(..));
                    let form_name = if immutable { "def" } else { "var" };

                    if let Some(name) = name_syntax.as_symbol() {
                        let entry = self.prebind_simple(
                            name,
                            name_syntax,
                            value_syntax,
                            immutable,
                            &mut seen_names,
                        );
                        entries.push(entry);
                    } else if Self::is_destructure_pattern(name_syntax) {
                        let entry = self.prebind_destructure(
                            name_syntax,
                            value_syntax,
                            immutable,
                            &mut seen_names,
                        );
                        entries.push(entry);
                    } else {
                        return Err(format!(
                            "{}: {} name must be a symbol or destructure pattern",
                            name_syntax.span, form_name
                        ));
                    }
                }
                FileForm::Signal(keyword_syntax) => {
                    let keyword = match &keyword_syntax.kind {
                        crate::syntax::SyntaxKind::Keyword(k) => k.clone(),
                        _ => {
                            return Err(format!(
                                "{}: signal requires a keyword argument, got {}",
                                keyword_syntax.span,
                                keyword_syntax.kind_label()
                            ));
                        }
                    };
                    self.declare_signal(&keyword, &keyword_syntax.span)?;
                    // Signal declarations produce the keyword value.
                    // Create a gensym binding whose initializer is the keyword literal.
                    let gensym_name = format!("__signal_{}", gensym_counter);
                    gensym_counter += 1;
                    let binding = self.bind(&gensym_name, &[], BindingScope::Local);
                    self.arena.get_mut(binding).is_prebound = true;
                    self.arena.get_mut(binding).is_synthetic = true;
                    entries.push(PreBound::Simple {
                        binding,
                        value_syntax: keyword_syntax,
                        deferred: None,
                    });
                }
                FileForm::Expr(expr_syntax) => {
                    let gensym_name = format!("__file_expr_{}", gensym_counter);
                    gensym_counter += 1;
                    let binding = self.bind(&gensym_name, &[], BindingScope::Local);
                    self.arena.get_mut(binding).is_prebound = true;
                    self.arena.get_mut(binding).is_synthetic = true;
                    entries.push(PreBound::Simple {
                        binding,
                        value_syntax: expr_syntax,
                        deferred: None,
                    });
                }
            }
        }

        // Snapshot scope after Pass 1: contains only pre-bound def/var names.
        // Used by Pass 3 to isolate fixpoint re-analysis from any bindings
        // that Pass 2 adds while analyzing expression entries.
        let pass1_scope_snapshot = self.scopes.last().map(|s| s.bindings.clone());

        // Pass 2: analyze all initializers sequentially.
        let mut bindings = Vec::new();
        let mut signal = Signal::silent();
        let mut last_binding: Option<Binding> = None;
        // Track lambda bindings for fixpoint signal propagation (Pass 3).
        // Each entry: (index in `bindings`, binding, reference to value syntax).
        let mut lambda_entries: Vec<(usize, Binding, &Syntax)> = Vec::new();

        for entry in &entries {
            match entry {
                PreBound::Simple {
                    binding,
                    value_syntax,
                    deferred,
                } => {
                    // Self-recursion context: a self-edge inside a top-level `def`'s
                    // lambda classifies `CaptureKind::Recursive` (the whole-module
                    // `%file-body` thunk makes `in_lambda` true, so such a binding
                    // resolves its self-edge to the executing closure — cell-free — like a
                    // nested self-recursive one).
                    let value = self.analyze_initializer(*binding, value_syntax)?;
                    // Register deferred (duplicate-name) bindings AFTER analyzing
                    // the RHS so the RHS sees the previous binding, not the new
                    // uninitialized one.
                    if let Some((name, scopes)) = deferred {
                        self.register_binding(name, scopes, *binding);
                    }
                    signal = signal.combine(value.signal);

                    let bindings_idx = bindings.len();
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_signals,
                        ..
                    } = &value.kind
                    {
                        self.signal_env.insert(*binding, *inferred_signals);
                        let arity = Arity::for_lambda(
                            rest_param.is_some(),
                            *num_required,
                            lambda_params.len(),
                        );
                        self.arity_env.insert(*binding, arity);
                        lambda_entries.push((bindings_idx, *binding, *value_syntax));
                    }
                    self.apply_transient_binding_state(*binding);

                    bindings.push((*binding, value));
                    last_binding = Some(*binding);
                }
                PreBound::Destructure {
                    pattern_syntax,
                    value_syntax,
                    immutable,
                    leaf_bindings,
                    deferred_leaves,
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    // Register deferred leaves AFTER analyzing the RHS (same
                    // reasoning as the Simple case above).
                    for (name, scopes, binding) in deferred_leaves {
                        self.register_binding(name, scopes, *binding);
                    }
                    signal = signal.combine(value.signal);

                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        *immutable,
                        &span,
                    )?;
                    self.pre_bindings.clear();

                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::silent(HirKind::Nil, span.clone())));
                        last_binding = Some(*leaf_binding);
                    }

                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));

                    let destructure_hir = Hir::silent(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::silent(HirKind::Var(tmp), span.clone())),
                            strict: true,
                        },
                        span.clone(),
                    );
                    let destr_gensym = format!("__file_destr_{}", gensym_counter);
                    gensym_counter += 1;
                    let destr_binding = self.bind(&destr_gensym, &[], BindingScope::Local);
                    self.arena.get_mut(destr_binding).is_synthetic = true;
                    bindings.push((destr_binding, destructure_hir));
                }
            }
        }

        // Pass 3: fixpoint loop for signal propagation through mutual recursion.
        //
        // Pass 2 analyzes bindings sequentially, so a lambda analyzed early may
        // see stale (optimistic) signals for lambdas analyzed later. For mutually
        // recursive functions, this means signals don't propagate through cycles:
        //
        //   (def foo (fn [] (bar)))    # analyzed first, sees bar as Pure (stale)
        //   (def bar (fn [] (yield 1) (foo)))  # analyzed second, correctly Yields
        //
        // foo stays Pure even though it calls a Yields function. Fix: re-analyze
        // lambda bindings until signal_env stabilizes.
        //
        // Scope isolation: Pass 2 may have analyzed expression entries (e.g.,
        // parameterize bodies) that contain `def` forms. Those defs register
        // bindings in the file scope. Re-analyzing lambda defs with these
        // extra bindings visible would produce incorrect capture sets. We
        // snapshot the scope before Pass 3 and restore it after so that
        // re-analysis sees only the pre-bound def/var names from Pass 1.
        //
        // Re-analysis side signals are benign: the side signals of re-analyzing
        // a lambda (additional `mark_captured()`, `mark_mutated()` calls on
        // bindings) are monotonic — they only add flags, never remove them.
        // Re-analysis can only make the result more conservative, never incorrect.
        if !lambda_entries.is_empty() {
            // Swap in the Pass 1 scope snapshot so re-analysis doesn't see
            // bindings added by expression entries during Pass 2.
            let mut pass2_bindings = None;
            if let (Some(snapshot), Some(scope)) = (&pass1_scope_snapshot, self.scopes.last_mut()) {
                pass2_bindings = Some(std::mem::replace(&mut scope.bindings, snapshot.clone()));
            }

            // Before fixpoint re-analysis, save Pass 2 errors and clear —
            // re-analysis may re-accumulate the same errors inside lambda
            // bodies, and we only want the final iteration's set for
            // lambdas. Non-lambda (def/var) errors from Pass 2 are
            // preserved and merged back below.
            let pass2_errors = std::mem::take(&mut self.errors);
            const MAX_FIXPOINT_ITERS: usize = 10;
            for _ in 0..MAX_FIXPOINT_ITERS {
                let mut changed = false;
                for &(idx, binding, value_syntax) in &lambda_entries {
                    let old_signal = self
                        .signal_env
                        .get(&binding)
                        .copied()
                        .unwrap_or_else(Signal::silent);
                    // Re-analysis rebuilds the capture set, so the self-recursion
                    // context must be set here too — else the final captures (from
                    // this pass) would lose the `Recursive` classification Pass 2 made.
                    let new_hir = self.analyze_initializer(binding, value_syntax)?;
                    if let HirKind::Lambda {
                        inferred_signals, ..
                    } = &new_hir.kind
                    {
                        if *inferred_signals != old_signal {
                            self.signal_env.insert(binding, *inferred_signals);
                            changed = true;
                        }
                    }
                    bindings[idx].1 = new_hir;
                }
                if !changed {
                    break;
                }
            }

            // Restore the full scope (with Pass 2 additions) for the body.
            if let (Some(saved), Some(scope)) = (pass2_bindings, self.scopes.last_mut()) {
                scope.bindings = saved;
            }

            // Merge Pass 2 errors (from non-lambda entries) with the
            // final-iteration lambda errors. Without this, non-lambda
            // errors (e.g. `(def x (some-undef))`) would be silently
            // dropped and their poison nodes would surface later as
            // "internal: error poison node in lowerer".
            let mut merged = pass2_errors;
            merged.extend(std::mem::take(&mut self.errors));
            self.errors = merged;
        }

        // Compute signal projection from the last binding's init value.
        // This must happen before pop_scope so signal_env is still populated.
        let projection = bindings
            .last()
            .and_then(|(_, value)| self.compute_signal_projection(value));

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::silent(HirKind::Var(binding), span.clone()),
            None => Hir::silent(HirKind::Nil, span.clone()),
        };

        self.pop_scope();

        // Stash the projection on the Analyzer for the pipeline to retrieve.
        self.last_signal_projection = projection;

        // Mark every direct file-letrec binding as MODULE-SCOPE. These are the
        // program/module-extent names (top-level `def`/`var`/expr statements and
        // their destructure leaves); their lifetime is the file-letrec scope, not
        // a per-activation scope exit. The region solver keys the reassigned-mutable
        // container classification off this so a top-level mutable behaves
        // identically whether the file-letrec is the outermost code object
        // (`elle FILE`) or the body of the synthetic `%file-body` whole-module thunk
        // (`elle test`), where `in_lambda` is spuriously true (the thunk wrapper).
        for (b, _) in &bindings {
            self.arena.get_mut(*b).is_file_scope = true;
        }

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(body),
            },
            span,
            signal,
        ))
    }
}
