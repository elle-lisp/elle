// audited: 2026-11-19
//! The pass over a file's top-level forms: each analyzed in order, into one letrec.
//!
//! docs/pipeline.md

use super::super::def::DefBound;
use super::*;

impl<'a> Analyzer<'a> {
    /// Analyze a file's top-level forms in order, as one letrec. Each `def`
    /// binds its name at its own form, pre-bound: the letrec holds a cell for
    /// a binding a lambda captures before any initializer runs. The letrec
    /// body names the last binding a form declares, and an empty file is
    /// `nil`. `analyze_file` settles the signals after.
    ///
    /// A `letrec` that ends the file holds the rest of the file, so its names
    /// live as long as the file's. It binds them in the file's letrec, in a
    /// scope of their own, and its body's forms are the file's forms after it.
    pub(in crate::hir::analyze) fn analyze_file_letrec(
        &mut self,
        forms: Vec<FileForm>,
        span: Span,
    ) -> Result<Hir, String> {
        let mut forms = super::nested::splice_begins(forms);
        if forms.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        // The file's top-level bindings are a macro definition environment:
        // template free variables must resolve here past use-site shadows.
        self.push_definition_scope();
        let mut frames = 1;

        // A signal declaration names a keyword for the whole compile, so every
        // form sees it, whatever its place in the file.
        for form in &forms {
            if let FileForm::Signal(keyword_syntax) = form {
                self.declare_file_signal(keyword_syntax)?;
            }
        }

        let mut bindings = Vec::new();
        let mut signal = Signal::silent();
        let mut last_binding: Option<Binding> = None;
        let mut gensym_counter = 0u32;

        let mut i = 0;
        while i < forms.len() {
            let form = forms[i];
            i += 1;
            let (whole, target, value) = match form {
                FileForm::Def(whole, target, value) | FileForm::Var(whole, target, value) => {
                    (whole, target, value)
                }
                FileForm::Expr(syntax) if i == forms.len() && is_letrec(syntax) => {
                    let items = syntax.as_list().unwrap_or_default();
                    let pairs = Self::letrec_bindings(items, syntax.span)?;
                    self.push_definition_scope();
                    frames += 1;
                    // A `def` in an initializer stands in the `letrec` form.
                    let at = syntax.span;
                    let group =
                        self.in_def_value(Some(*syntax), |this| this.bind_letrec(pairs, at))?;
                    signal = signal.combine(group.signal);
                    bindings.extend(group.bindings);
                    for (pattern, tmp) in group.destructures {
                        let value = Hir::silent(HirKind::Var(tmp), at);
                        let destructure = self.destructure(pattern, value, true, at);
                        signal = signal.combine(destructure.signal);
                        let step = self.file_gensym("__file_destr_", &mut gensym_counter);
                        bindings.push((step, destructure));
                    }
                    forms = super::nested::splice_begins(
                        items[2..].iter().map(classify_form).collect(),
                    );
                    i = 0;
                    continue;
                }
                // A signal declaration yields its keyword, and an expression
                // its value, each under a name of its own.
                FileForm::Signal(syntax) | FileForm::Expr(syntax) => {
                    let prefix = match form {
                        FileForm::Signal(_) => {
                            self.declare_file_signal(syntax)?;
                            "__signal_"
                        }
                        _ => "__file_expr_",
                    };
                    let binding = self.file_gensym(prefix, &mut gensym_counter);
                    self.arena.get_mut(binding).is_prebound = true;
                    let value =
                        self.in_def_value(None, |this| this.analyze_definition(binding, syntax))?;
                    signal = signal.combine(value.signal);
                    bindings.push((binding, value));
                    last_binding = Some(binding);
                    continue;
                }
            };
            let form_is_def = matches!(form, FileForm::Def(..));
            // A `def` in the value stands in the form (`placement.rs`).
            let bound = self.in_def_value(Some(*whole), |this| {
                this.bind_def(target, value, form_is_def, true, span)
            })?;
            match bound {
                DefBound::Name(binding, value) => {
                    signal = signal.combine(value.signal);
                    bindings.push((binding, value));
                    last_binding = Some(binding);
                }
                DefBound::Pattern(pattern, value) => {
                    signal = signal.combine(value.signal);
                    // Each leaf holds nil before the destructure sets it.
                    for leaf in &pattern.bindings().bindings {
                        bindings.push((*leaf, Hir::silent(HirKind::Nil, span)));
                        last_binding = Some(*leaf);
                    }
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    let destructure =
                        self.destructure(pattern, Hir::silent(HirKind::Var(tmp), span), true, span);
                    signal = signal.combine(destructure.signal);
                    let step = self.file_gensym("__file_destr_", &mut gensym_counter);
                    bindings.push((step, destructure));
                }
            }
        }

        // Body: reference to the last binding (the file's return value).
        let body = match last_binding {
            Some(binding) => Hir::silent(HirKind::Var(binding), span),
            None => Hir::silent(HirKind::Nil, span),
        };

        for _ in 0..frames {
            self.pop_scope();
        }

        // Mark every direct file-letrec binding as module scope: a top-level
        // `def`, `var` or expression statement, a binding of a `letrec` that
        // ends the file, a destructure leaf, or a temporary. Its lifetime is
        // the file-letrec, also when the file-letrec is the body of the
        // `%file-body` whole-module thunk (`BindingInner::is_file_scope`).
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

    /// Declare the signal a file's `(signal :kw)` names. Declaring a keyword
    /// again keeps its bit.
    fn declare_file_signal(&mut self, keyword_syntax: &Syntax) -> Result<(), String> {
        let SyntaxKind::Keyword(keyword) = &keyword_syntax.kind else {
            return Err(format!(
                "{}: signal requires a keyword argument, got {}",
                keyword_syntax.span,
                keyword_syntax.kind_label()
            ));
        };
        self.declare_signal(keyword, &keyword_syntax.span)
    }
}

/// Whether `syntax` is a `letrec` form.
fn is_letrec(syntax: &Syntax) -> bool {
    syntax
        .as_list()
        .and_then(<[Syntax]>::first)
        .and_then(Syntax::as_symbol)
        == Some("letrec")
}
