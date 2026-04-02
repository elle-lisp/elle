//! File-scope letrec compilation for top-level forms.

use super::*;
use crate::syntax::{ScopeId, Syntax};

/// Intermediate classification for file-scope letrec Pass 1.
/// Each form is pre-bound as either a simple name or a destructure pattern.
enum PreBound<'s> {
    Simple {
        binding: Binding,
        value_syntax: &'s Syntax,
        /// Name and scopes for deferred bindings (duplicate names).
        /// When set, Pass 2 registers this binding in the scope
        /// before analyzing the value, achieving sequential shadowing.
        deferred: Option<(String, Vec<ScopeId>)>,
    },
    Destructure {
        pattern_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        /// Pre-created bindings from pass 1, keyed by name.
        /// Passed to `analyze_destructure_pattern` in pass 2 to
        /// ensure binding identity matches.
        leaf_bindings: HashMap<String, Binding>,
        /// Leaf bindings that were deferred (duplicate names).
        /// Maps name → (scopes, binding) for registration in Pass 2.
        deferred_leaves: Vec<(String, Vec<ScopeId>, Binding)>,
    },
}

impl<'a> Analyzer<'a> {
    /// Analyze a list of top-level forms as a synthetic letrec.
    ///
    /// Each form is classified as `Def` (immutable), `Var` (mutable), or
    /// `Expr` (gensym-named dummy binding). Three-pass analysis:
    /// - Pass 1: pre-bind all names (enables mutual recursion)
    /// - Pass 2: analyze initializers sequentially
    /// - Pass 3: fixpoint loop for signal propagation through mutual recursion
    ///
    /// Returns a single `HirKind::Letrec` node. The body is a reference
    /// to the last binding (the file's return value).
    pub(crate) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        if forms.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        self.push_scope(false);

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
                    crate::signals::registry::global_registry()
                        .lock()
                        .unwrap()
                        .register(&keyword)
                        .map_err(|e| format!("{}: {}", keyword_syntax.span, e))?;
                    // Signal declarations produce the keyword value.
                    // Create a gensym binding whose initializer is the keyword literal.
                    let gensym_name = format!("__signal_{}", gensym_counter);
                    gensym_counter += 1;
                    let binding = self.bind(&gensym_name, &[], BindingScope::Local);
                    self.arena.get_mut(binding).is_prebound = true;
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
                    if let Some((name, scopes)) = deferred {
                        self.register_binding(name, scopes, *binding);
                    }
                    let value = self.analyze_expr(value_syntax)?;
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
                    for (name, scopes, binding) in deferred_leaves {
                        self.register_binding(name, scopes, *binding);
                    }
                    let value = self.analyze_expr(value_syntax)?;
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

            const MAX_FIXPOINT_ITERS: usize = 10;
            for _ in 0..MAX_FIXPOINT_ITERS {
                let mut changed = false;
                for &(idx, binding, value_syntax) in &lambda_entries {
                    let old_signal = self
                        .signal_env
                        .get(&binding)
                        .copied()
                        .unwrap_or_else(Signal::silent);
                    let new_hir = self.analyze_expr(value_syntax)?;
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
        }

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::silent(HirKind::Var(binding), span.clone()),
            None => Hir::silent(HirKind::Nil, span.clone()),
        };

        self.pop_scope();

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(body),
            },
            span,
            signal,
        ))
    }

    /// Pass 1 helper: pre-bind a simple (non-destructuring) name for file-scope letrec.
    ///
    /// Creates the binding and seeds signal/arity tracking for lambda forms.
    /// Duplicate names are deferred to Pass 2 for sequential shadowing.
    fn prebind_simple<'s>(
        &mut self,
        name: &str,
        name_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        seen_names: &mut HashSet<String>,
    ) -> PreBound<'s> {
        let is_duplicate = !seen_names.insert(name.to_string());
        let (binding, deferred) = if is_duplicate {
            // Duplicate name: create binding but don't register in scope yet.
            // Pass 2 will register it via register_binding for sequential shadowing.
            let sym = self.symbols.intern(name);
            let b = self.arena.alloc(sym, BindingScope::Local);
            (b, Some((name.to_string(), name_syntax.scopes.clone())))
        } else {
            let b = self.bind(name, name_syntax.scopes.as_slice(), BindingScope::Local);
            (b, None)
        };

        self.arena.get_mut(binding).is_prebound = true;
        if immutable {
            self.arena.get_mut(binding).is_immutable = true;
        }

        // Seed signal_env and arity_env for lambda forms so self-recursive
        // calls don't default to Yields during analysis.
        if Self::is_lambda_syntax(value_syntax) {
            self.signal_env.insert(binding, Signal::silent());
            if let Some(list) = value_syntax.as_list() {
                if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
                    self.arity_env
                        .insert(binding, Self::arity_from_syntax_params(params_syn));
                }
            }
        }

        PreBound::Simple {
            binding,
            value_syntax,
            deferred,
        }
    }

    /// Pass 1 helper: pre-bind destructure leaf names for file-scope letrec.
    ///
    /// Extracts leaf names from the pattern and creates bindings for each.
    /// Duplicate names are deferred to Pass 2 for sequential shadowing.
    fn prebind_destructure<'s>(
        &mut self,
        pattern_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        seen_names: &mut HashSet<String>,
    ) -> PreBound<'s> {
        let mut names = Vec::new();
        Self::extract_pattern_names(pattern_syntax, &mut names);
        let mut leaf_bindings = HashMap::new();
        let mut deferred_leaves = Vec::new();

        for (name, name_scopes) in &names {
            if *name == "_" {
                continue;
            }
            let is_dup = !seen_names.insert(name.to_string());
            let b = if is_dup {
                // Duplicate: create binding without scope registration.
                // register_binding in Pass 2 handles slot allocation.
                let sym = self.symbols.intern(name);
                let b = self.arena.alloc(sym, BindingScope::Local);
                deferred_leaves.push((name.to_string(), name_scopes.to_vec(), b));
                b
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };
            self.arena.get_mut(b).is_prebound = true;
            if immutable {
                self.arena.get_mut(b).is_immutable = true;
            }
            leaf_bindings.insert(name.to_string(), b);
        }

        PreBound::Destructure {
            pattern_syntax,
            value_syntax,
            immutable,
            leaf_bindings,
            deferred_leaves,
        }
    }

    /// Check if a syntax node is a lambda form: `(fn ...)`.
    fn is_lambda_syntax(syntax: &Syntax) -> bool {
        if let Some(list) = syntax.as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn")
        } else {
            false
        }
    }
}
