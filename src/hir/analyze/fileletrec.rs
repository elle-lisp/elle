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
    /// - Pass 3: fixpoint loop for effect propagation through mutual recursion
    ///
    /// Returns a single `HirKind::Letrec` node. The body is a reference
    /// to the last binding (the file's return value).
    pub(crate) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        if forms.is_empty() {
            return Ok(Hir::inert(HirKind::Nil, span));
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
                FileForm::Effect(keyword_syntax) => {
                    let keyword = match &keyword_syntax.kind {
                        crate::syntax::SyntaxKind::Keyword(k) => k.clone(),
                        _ => {
                            return Err(format!(
                                "{}: effect requires a keyword argument, got {}",
                                keyword_syntax.span,
                                keyword_syntax.kind_label()
                            ));
                        }
                    };
                    crate::effects::registry::global_registry()
                        .lock()
                        .unwrap()
                        .register(&keyword)
                        .map_err(|e| format!("{}: {}", keyword_syntax.span, e))?;
                    // Effect declarations produce the keyword value.
                    // Create a gensym binding whose initializer is the keyword literal.
                    let gensym_name = format!("__effect_{}", gensym_counter);
                    gensym_counter += 1;
                    let binding = self.bind(&gensym_name, &[], BindingScope::Local);
                    binding.mark_prebound();
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
                    binding.mark_prebound();
                    entries.push(PreBound::Simple {
                        binding,
                        value_syntax: expr_syntax,
                        deferred: None,
                    });
                }
            }
        }

        // Pass 2: analyze all initializers sequentially.
        let mut bindings = Vec::new();
        let mut effect = Effect::inert();
        let mut last_binding: Option<Binding> = None;
        // Track lambda bindings for fixpoint effect propagation (Pass 3).
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
                    effect = effect.combine(value.effect);

                    let bindings_idx = bindings.len();
                    if let HirKind::Lambda {
                        params: lambda_params,
                        num_required,
                        rest_param,
                        inferred_effects,
                        ..
                    } = &value.kind
                    {
                        self.effect_env.insert(*binding, *inferred_effects);
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
                    effect = effect.combine(value.effect);

                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        *immutable,
                        &span,
                    )?;
                    self.pre_bindings.clear();

                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::inert(HirKind::Nil, span.clone())));
                        last_binding = Some(*leaf_binding);
                    }

                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));

                    let destructure_hir = Hir::inert(
                        HirKind::Destructure {
                            pattern,
                            value: Box::new(Hir::inert(HirKind::Var(tmp), span.clone())),
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

        // Pass 3: fixpoint loop for effect propagation through mutual recursion.
        //
        // Pass 2 analyzes bindings sequentially, so a lambda analyzed early may
        // see stale (optimistic) effects for lambdas analyzed later. For mutually
        // recursive functions, this means effects don't propagate through cycles:
        //
        //   (def foo (fn [] (bar)))    # analyzed first, sees bar as Pure (stale)
        //   (def bar (fn [] (yield 1) (foo)))  # analyzed second, correctly Yields
        //
        // foo stays Pure even though it calls a Yields function. Fix: re-analyze
        // lambda bindings until effect_env stabilizes.
        //
        // Re-analysis side effects are benign: the side effects of re-analyzing
        // a lambda (additional `mark_captured()`, `mark_mutated()` calls on
        // bindings) are monotonic — they only add flags, never remove them.
        // Re-analysis can only make the result more conservative, never incorrect.
        if !lambda_entries.is_empty() {
            const MAX_FIXPOINT_ITERS: usize = 10;
            for _ in 0..MAX_FIXPOINT_ITERS {
                let mut changed = false;
                for &(idx, binding, value_syntax) in &lambda_entries {
                    let old_effect = self
                        .effect_env
                        .get(&binding)
                        .copied()
                        .unwrap_or_else(Effect::inert);
                    let new_hir = self.analyze_expr(value_syntax)?;
                    if let HirKind::Lambda {
                        inferred_effects, ..
                    } = &new_hir.kind
                    {
                        if *inferred_effects != old_effect {
                            self.effect_env.insert(binding, *inferred_effects);
                            changed = true;
                        }
                    }
                    bindings[idx].1 = new_hir;
                }
                if !changed {
                    break;
                }
            }
        }

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::inert(HirKind::Var(binding), span.clone()),
            None => Hir::inert(HirKind::Nil, span.clone()),
        };

        self.pop_scope();

        Ok(Hir::new(
            HirKind::Letrec {
                bindings,
                body: Box::new(body),
            },
            span,
            effect,
        ))
    }

    /// Pass 1 helper: pre-bind a simple (non-destructuring) name for file-scope letrec.
    ///
    /// Creates the binding and seeds effect/arity tracking for lambda forms.
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
            let b = Binding::new(sym, BindingScope::Local);
            (b, Some((name.to_string(), name_syntax.scopes.clone())))
        } else {
            let b = self.bind(name, name_syntax.scopes.as_slice(), BindingScope::Local);
            (b, None)
        };

        binding.mark_prebound();
        if immutable {
            binding.mark_immutable();
        }

        // Seed effect_env and arity_env for lambda forms so self-recursive
        // calls don't default to Yields during analysis.
        if Self::is_lambda_syntax(value_syntax) {
            self.effect_env.insert(binding, Effect::inert());
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
                let b = Binding::new(sym, BindingScope::Local);
                deferred_leaves.push((name.to_string(), name_scopes.to_vec(), b));
                b
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };
            b.mark_prebound();
            if immutable {
                b.mark_immutable();
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
