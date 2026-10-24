// audited: 2026-10-24
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

        // Pass 2: analyze all initializers sequentially.
        let mut bindings = Vec::new();
        let mut signal = Signal::silent();
        let mut last_binding: Option<Binding> = None;

        for entry in &entries {
            match entry {
                PreBound::Simple {
                    binding,
                    value_syntax,
                    deferred,
                } => {
                    // `analyze_initializer` sets the self-recursion context, so a
                    // self-edge inside a top-level `def`'s lambda classifies
                    // `CaptureKind::Recursive`.
                    let value = self.analyze_initializer(*binding, value_syntax)?;
                    // Register deferred (duplicate-name) bindings AFTER analyzing
                    // the RHS so the RHS sees the previous binding, not the new
                    // uninitialized one.
                    if let Some((name, scopes)) = deferred {
                        self.register_binding(name, scopes, *binding);
                    }
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
                        bindings.push((*leaf_binding, Hir::silent(HirKind::Nil, span)));
                        last_binding = Some(*leaf_binding);
                    }

                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));

                    let destructure_hir =
                        self.destructure(pattern, Hir::silent(HirKind::Var(tmp), span), true, span);
                    signal = signal.combine(destructure_hir.signal);
                    let destr_gensym = format!("__file_destr_{}", gensym_counter);
                    gensym_counter += 1;
                    let destr_binding = self.bind(&destr_gensym, &[], BindingScope::Local);
                    self.arena.get_mut(destr_binding).is_synthetic = true;
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
