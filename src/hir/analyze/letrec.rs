//! The `letrec` binding form analyzer.

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_letrec(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: letrec requires bindings and body", span));
        }

        let bindings_syntax = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::ArrayMut(_)) {
                format!(
                    "{}: letrec bindings must use (...) or [...], not @[...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: letrec bindings must be a list (...) or [...], got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;

        self.push_scope(false);

        // Pass 1: Classify each binding. Pre-bind ALL names for mutual
        // visibility — simple symbols AND destructure leaf names.
        // Destructure leaf names are pre-bound so that other initializers
        // (e.g., recursive functions) can reference them.
        //
        // Duplicate names are rejected at compile time.
        //
        // The double-binding problem: analyze_destructure_pattern in pass 2
        // also calls self.bind() for the same names. To prevent creating
        // duplicate Binding objects, analyze_destructure_pattern checks
        // lookup_in_current_scope for the Local scope case, reusing
        // pre-existing bindings.
        //
        // Bindings are flat pairs: [name1 value1 name2 value2 ...]
        enum LetrecEntry<'s> {
            Simple(Binding, &'s Syntax),
            Destructure {
                pattern: &'s Syntax,
                value: &'s Syntax,
                leaf_bindings: HashMap<String, Binding>,
            },
        }
        let mut entries = Vec::new();
        let mut duplicates = super::scopes::DuplicateGuard::default();

        if bindings_syntax.len() % 2 != 0 {
            return Err(format!(
                "{}: letrec bindings must have an even number of forms (name/value pairs)",
                span
            ));
        }

        let mut i = 0;
        while i < bindings_syntax.len() {
            let name_syn = &bindings_syntax[i];
            let value_syn = &bindings_syntax[i + 1];

            if let Some(raw_name) = name_syn.as_symbol() {
                // Simple binding — bind immediately for mutual recursion.
                // Marked prebound: may be captured before initialization.
                // Bound with the name's hygiene scopes (like fileletrec and
                // analyze_begin), so a macro-template binder and a user
                // binder of the same spelling are distinct identities.
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let sym = self.symbols.intern(name);
                duplicates.check(sym, name, name_syn.scopes.as_slice(), &name_syn.span)?;
                let b = self.bind(name, name_syn.scopes.as_slice(), BindingScope::Local);
                let fn_depth = self.fn_depth;
                let inner = self.arena.get_mut(b);
                inner.is_prebound = true;
                inner.init_pending = true;
                inner.prebind_fn_depth = fn_depth;
                if self.immutable_by_default && !is_mutable {
                    self.arena.get_mut(b).is_immutable = true;
                }
                entries.push(LetrecEntry::Simple(b, value_syn));
            } else if Self::is_destructure_pattern(name_syn) {
                // Destructure pattern — pre-bind leaf names for mutual visibility
                let mut names = Vec::new();
                Self::extract_pattern_names(name_syn, &mut names);
                let mut leaf_bindings = HashMap::new();
                for (name, name_scopes) in &names {
                    if *name != "_" {
                        let sym = self.symbols.intern(name);
                        duplicates.check(sym, name, name_scopes, &name_syn.span)?;
                        let b = self.bind(name, name_scopes, BindingScope::Local);
                        let fn_depth = self.fn_depth;
                        let inner = self.arena.get_mut(b);
                        inner.is_prebound = true;
                        inner.init_pending = true;
                        inner.prebind_fn_depth = fn_depth;
                        // Immutability set later by analyze_destructure_pattern
                        leaf_bindings.insert(name.to_string(), b);
                    }
                }
                entries.push(LetrecEntry::Destructure {
                    pattern: name_syn,
                    value: value_syn,
                    leaf_bindings,
                });
            } else {
                return Err(format!(
                    "{}: letrec binding name must be a symbol or destructure pattern",
                    span
                ));
            }
            i += 2;
        }

        // Second pass: analyze values and build the output.
        // Simple bindings go into the Letrec node's bindings vec.
        // Destructured bindings: the temp binding AND all leaf bindings
        // go into the Letrec bindings vec (leaf bindings initialized to
        // nil). This ensures the lowerer allocates slots for all bindings
        // before lowering any lambda values — lambdas may capture
        // destructured leaf bindings. Destructure nodes in the body then
        // update the leaf binding slots.
        //
        // Seed signal_env for all pre-bound simple bindings with Silent.
        // Without this, forward-referenced letrec siblings default to
        // Signal::yields() (the unknown-binding fallback in
        // get_raw_callee_signal), causing spurious SuspendingCall
        // instructions. This matches analyze_file_letrec's optimistic
        // seeding strategy.
        for entry in &entries {
            if let LetrecEntry::Simple(binding, _) = entry {
                self.signal_env.insert(*binding, Signal::silent());
            }
        }

        let mut bindings = Vec::new();
        let mut destructures = Vec::new();
        let mut signal = Signal::silent();
        // Lambda bindings, for the fixpoint below: (index in `bindings`,
        // binding, its initializer syntax).
        let mut lambda_entries: Vec<(usize, Binding, &Syntax)> = Vec::new();

        for entry in &entries {
            match entry {
                LetrecEntry::Simple(binding, value_syntax) => {
                    // Set the self-recursion context to this binding so a self-edge
                    // inside its lambda classifies `CaptureKind::Recursive`.
                    let value = self.analyze_initializer(*binding, value_syntax)?;
                    // Initializer analyzed: later initializers and the body
                    // may now read this binding's value.
                    self.arena.get_mut(*binding).init_pending = false;
                    // Track signal and arity for interprocedural analysis
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
                        lambda_entries.push((bindings.len(), *binding, value_syntax));
                    }
                    self.apply_transient_binding_state(*binding);
                    bindings.push((*binding, value));
                }
                LetrecEntry::Destructure {
                    pattern: pattern_syntax,
                    value: value_syntax,
                    leaf_bindings,
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    // Create a temp binding for the value in the Letrec bindings
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    // Analyze the pattern using pre-created bindings from pass 1
                    // Immutable by default; individual leaves with @ opt into mutability
                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        self.immutable_by_default,
                        &span,
                    )?;
                    self.pre_bindings.clear();
                    // Initializer analyzed: the destructured leaves are
                    // initialized once the pattern runs.
                    for leaf in leaf_bindings.values() {
                        self.arena.get_mut(*leaf).init_pending = false;
                    }
                    // Add leaf bindings to the Letrec bindings vec (initialized
                    // to nil) so the lowerer allocates slots for them before
                    // lowering any lambda values that might capture them.
                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::silent(HirKind::Nil, span.clone())));
                    }
                    destructures.push((pattern, tmp));
                }
            }
        }

        // Signals converge by fixpoint before anything reads them.
        //
        // The pass above analyzes initializers in order against a seed of
        // `Signal::silent()`, so a lambda analyzed early reads whatever the
        // seed says about a sibling analyzed later. In a mutual cycle that
        // sibling is the one carrying the signal, and the seed is the bottom of
        // the lattice — the early lambda therefore comes out too LOW. Too low is
        // the direction that costs a guarantee: a function that reaches `:error`
        // through the cycle reads as silent, satisfies a compile-time
        // `(silence)`, and aborts at runtime instead of failing to compile.
        //
        // Re-analyze the lambdas against the signals now recorded, and repeat
        // until an iteration changes nothing. Each pass can only move a signal
        // up the lattice, so this terminates on lattice height; the iteration
        // cap is a guard against a lattice bug, not part of the argument.
        //
        // Only lambdas are re-analyzed. A destructure entry mints a fresh temp
        // binding and fresh pattern leaves every time it runs, so replaying one
        // would add duplicate bindings rather than refine a signal.
        //
        // See docs/pipeline.md § The fixpoint loop.
        if !lambda_entries.is_empty() {
            // Re-analysis walks the same lambda bodies again and re-reports
            // whatever they raise. Keep the errors from this pass aside and
            // merge them back after, so only the final iteration's copy of a
            // lambda-body error survives.
            let pass_errors = std::mem::take(&mut self.errors);
            const MAX_FIXPOINT_ITERS: usize = 10;
            for _ in 0..MAX_FIXPOINT_ITERS {
                let mut changed = false;
                for &(idx, binding, value_syntax) in &lambda_entries {
                    let old = self
                        .signal_env
                        .get(&binding)
                        .copied()
                        .unwrap_or_else(Signal::silent);
                    self.errors.clear();
                    // Re-analysis rebuilds the capture set, so this must go
                    // through `analyze_initializer` — it sets the self-recursion
                    // context that classifies a self-edge as `Recursive`.
                    let new_hir = self.analyze_initializer(binding, value_syntax)?;
                    if let HirKind::Lambda {
                        inferred_signals, ..
                    } = &new_hir.kind
                    {
                        if *inferred_signals != old {
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
            let mut merged = pass_errors;
            merged.extend(std::mem::take(&mut self.errors));
            self.errors = merged;
        }

        // The body reads the converged signals, so it is analyzed last.
        let body = self.analyze_body(&items[2..], span.clone())?;

        // Aggregate from the post-fixpoint HIR. Accumulating during the pass
        // above would bake in the pre-convergence values.
        for (_, value) in &bindings {
            signal = signal.combine(value.signal);
        }
        signal = signal.combine(body.signal);

        self.pop_scope();

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = destructures
                .into_iter()
                .map(|(pattern, tmp)| {
                    Hir::silent(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::silent(HirKind::Var(tmp), span.clone())),
                            strict: true,
                        },
                        span.clone(),
                    )
                })
                .collect();
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span.clone(), signal)
        };

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(final_body),
            },
            span,
            signal,
        ))
    }
}
