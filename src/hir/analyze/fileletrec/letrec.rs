// audited: 2026-11-17
//! The two passes over a file's top-level forms: pre-bind every name, then
//! analyze each initializer in order.
//!
//! docs/pipeline.md

use super::*;

impl<'a> Analyzer<'a> {
    /// Analyze a file's top-level forms as one letrec. Pass 1 pre-binds every
    /// name, and Pass 2 analyzes the initializers in order. The letrec body
    /// names the last binding a form declares, and an empty file is `nil`.
    /// `analyze_file` settles the signals after.
    pub(in crate::hir::analyze) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        let forms = super::nested::splice_begins(forms);
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

        // Pass 1: pre-bind all names for mutual visibility. Every `def` of a
        // name agrees on mutability.
        for form in &forms {
            match form {
                FileForm::Def(whole, name_syntax, value_syntax)
                | FileForm::Var(whole, name_syntax, value_syntax) => {
                    let immutable = matches!(form, FileForm::Def(..));
                    let form_name = if immutable { "def" } else { "var" };

                    let entry = if let Some(name) = name_syntax.as_symbol() {
                        self.prebind_simple(
                            whole,
                            name,
                            name_syntax,
                            value_syntax,
                            immutable,
                            &mut seen_names,
                        )
                    } else if Self::is_destructure_pattern(name_syntax) {
                        self.prebind_destructure(
                            whole,
                            name_syntax,
                            value_syntax,
                            immutable,
                            &mut seen_names,
                        )
                    } else {
                        return Err(format!(
                            "{}: {} name must be a symbol or destructure pattern",
                            name_syntax.span, form_name
                        ));
                    };
                    self.declare_targets(name_syntax, immutable)?;
                    entries.push(entry);
                }
                FileForm::Signal(keyword_syntax) => {
                    let keyword = match &keyword_syntax.kind {
                        crate::syntax::SyntaxKind::Keyword(k) => *k,
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
                    let binding = self.file_gensym("__signal_", &mut gensym_counter);
                    self.arena.get_mut(binding).is_prebound = true;
                    entries.push(PreBound::Simple {
                        form: None,
                        binding,
                        value_syntax: keyword_syntax,
                        deferred: None,
                    });
                }
                FileForm::Expr(expr_syntax) => {
                    let binding = self.file_gensym("__file_expr_", &mut gensym_counter);
                    self.arena.get_mut(binding).is_prebound = true;
                    entries.push(PreBound::Simple {
                        form: None,
                        binding,
                        value_syntax: expr_syntax,
                        deferred: None,
                    });
                }
            }
        }

        // Pass 2: analyze all initializers sequentially.
        let mut bindings = Vec::new();
        let mut signal = Signal::silent();
        let mut last_binding: Option<Binding> = None;

        for entry in &entries {
            match entry {
                PreBound::Simple {
                    form,
                    binding,
                    value_syntax,
                    deferred,
                } => {
                    // A redefinition is `Defining` while its value is analyzed: a
                    // lambda in the value reads it, and a direct read reads the
                    // previous binding.
                    if let Some((name, scopes)) = deferred {
                        self.register_defining(name, scopes, *binding);
                    }
                    // `analyze_initializer` sets the self-recursion context, so a
                    // self-edge inside a top-level `def`'s lambda classifies
                    // `CaptureKind::Recursive`. A `def` in the value stands in
                    // the form (`placement.rs`).
                    let value = self.in_def_value(form.copied(), |this| {
                        this.analyze_initializer(*binding, value_syntax)
                    })?;
                    let preset = match deferred {
                        Some((name, scopes)) => {
                            let preset = self.preset(*binding, name, scopes, span)?;
                            self.reveal(name, *binding);
                            preset
                        }
                        None => None,
                    };
                    signal = signal.combine(value.signal);

                    self.record_lambda_facts(*binding, &value);
                    if let HirKind::Var(source) = &value.kind {
                        // `(def dynamic-eq %eq)` names a primitive, whose signal
                        // and arity are fixed, so a call through the new name
                        // signals what a call to the primitive does.
                        let source_inner = self.arena.get(*source);
                        if self.arena.get(*binding).is_immutable
                            && source_inner.is_primitive
                            && source_inner.is_immutable
                        {
                            if let Some(sig) = self.signal_env.get(source).copied() {
                                self.signal_env.insert(*binding, sig);
                            }
                            if let Some(arity) = self.arity_env.get(source).copied() {
                                self.arity_env.insert(*binding, arity);
                            }
                        }
                    }
                    self.apply_transient_binding_state(*binding);

                    match preset {
                        // The binding holds the previous value, and the value
                        // assigns it.
                        Some(init) => {
                            bindings.push((*binding, init));
                            let value_signal = value.signal;
                            let assign = HirKind::Assign {
                                target: *binding,
                                value: Box::new(value),
                            };
                            let assign = Hir::new(assign, span, value_signal);
                            let step = self.file_gensym("__file_expr_", &mut gensym_counter);
                            bindings.push((step, assign));
                        }
                        None => bindings.push((*binding, value)),
                    }
                    last_binding = Some(*binding);
                }
                PreBound::Destructure {
                    form,
                    pattern_syntax,
                    value_syntax,
                    immutable,
                    leaf_bindings,
                    deferred_leaves,
                } => {
                    // A redefined leaf is `Defining` while the value is analyzed,
                    // as in the Simple case above.
                    for (name, scopes, binding) in deferred_leaves {
                        self.register_defining(name, scopes, *binding);
                    }
                    let value =
                        self.in_def_value(Some(**form), |this| this.analyze_expr(value_syntax))?;
                    signal = signal.combine(value.signal);

                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        *immutable,
                        &span,
                    )?;
                    self.pre_bindings.clear();

                    // Each leaf holds nil before the destructure sets it, or the
                    // previous value where a lambda in the value read it.
                    let mut presets = HashMap::new();
                    for (name, scopes, binding) in deferred_leaves {
                        if let Some(init) = self.preset(*binding, name, scopes, span)? {
                            presets.insert(*binding, init);
                        }
                        self.reveal(name, *binding);
                    }
                    for leaf_binding in &pattern.bindings().bindings {
                        let init = presets
                            .remove(leaf_binding)
                            .unwrap_or_else(|| Hir::silent(HirKind::Nil, span));
                        bindings.push((*leaf_binding, init));
                        last_binding = Some(*leaf_binding);
                    }

                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));

                    let destructure_hir =
                        self.destructure(pattern, Hir::silent(HirKind::Var(tmp), span), true, span);
                    signal = signal.combine(destructure_hir.signal);
                    let destr_binding = self.file_gensym("__file_destr_", &mut gensym_counter);
                    bindings.push((destr_binding, destructure_hir));
                }
            }
        }

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::silent(HirKind::Var(binding), span),
            None => Hir::silent(HirKind::Nil, span),
        };

        self.pop_scope();

        // Mark every direct file-letrec binding as module scope: a top-level
        // `def`, `var` or expression statement, a destructure leaf, or a
        // temporary. Its lifetime is the file-letrec, also when the file-letrec
        // is the body of the `%file-body` whole-module thunk
        // (`BindingInner::is_file_scope`).
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
