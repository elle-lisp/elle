// audited: 2026-11-09
//! A file's spliced top-level `begin`, the `def` forms nested in its forms, and the scope each lambda entry saw.
//!
//! docs/bindings.md
//! docs/pipeline.md

use super::*;
use crate::hir::analyze::owned::reach;
use std::rc::Rc;

/// The file's scope as one lambda entry saw it when Pass 2 analyzed it.
/// Consecutive entries share a view until the file registers another name.
pub(super) type ScopeView = Rc<HashMap<SymbolId, Vec<ScopedBinding>>>;

/// The forms of `forms`, each top-level `begin` replaced by its own forms.
/// `begin` opens no scope, so what it holds stands at top level.
pub(super) fn splice_begins(forms: Vec<FileForm<'_>>) -> Vec<FileForm<'_>> {
    let mut out = Vec::with_capacity(forms.len());
    for form in forms {
        splice_into(form, &mut out);
    }
    out
}

fn splice_into<'s>(form: FileForm<'s>, out: &mut Vec<FileForm<'s>>) {
    if let FileForm::Expr(syntax) = &form {
        if let Some(items) = syntax.as_list() {
            if items.len() > 1 && matches!(items[0].as_symbol(), Some("begin" | "do")) {
                for item in &items[1..] {
                    splice_into(classify_form(item), out);
                }
                return;
            }
        }
    }
    out.push(form);
}

impl<'a> Analyzer<'a> {
    /// Pass 1 helper: declare each `def` nested in the top-level form
    /// `syntax`, and pre-bind the first `def` of each name the file has not
    /// bound, for the file `owner`. The analysis of the `def` binds a later
    /// one, visible after its value.
    pub(super) fn prebind_nested(
        &mut self,
        syntax: &Syntax,
        seen_names: &mut HashSet<String>,
        owner: usize,
    ) -> Result<(), String> {
        let mut reached = Vec::new();
        reach(syntax, false, &mut reached);
        seen_names.extend(self.forward_defs(reached, owner)?);
        Ok(())
    }

    /// The file's scope as it stands, shared with `last` while the file has
    /// registered no name since `last` was taken.
    pub(super) fn scope_view(&self, last: &mut Option<(u16, ScopeView)>) -> ScopeView {
        let scope = self
            .scopes
            .last()
            .expect("a file letrec analyzes in its own scope");
        if let Some((at, view)) = last {
            if *at == scope.next_local {
                return view.clone();
            }
        }
        let view = Rc::new(scope.bindings.clone());
        *last = Some((scope.next_local, view.clone()));
        view
    }

    /// Pass 3: converge the lambda entries' signals, each re-analyzed against
    /// the scope `views` holds for it, then restore the scope Pass 2 left.
    pub(super) fn converge_in_views(
        &mut self,
        entries: &[(Binding, &Syntax)],
        views: &[ScopeView],
    ) -> Result<Vec<Hir>, String> {
        let saved = self.scopes.last().map(|scope| scope.bindings.clone());
        let mut installed = None;
        let converged = self.converge_signals_in(entries, |analyzer, i| {
            let view = &views[i];
            if installed != Some(Rc::as_ptr(view)) {
                if let Some(scope) = analyzer.scopes.last_mut() {
                    scope.bindings = (**view).clone();
                }
                installed = Some(Rc::as_ptr(view));
            }
        });
        if let (Some(saved), Some(scope)) = (saved, self.scopes.last_mut()) {
            scope.bindings = saved;
        }
        converged
    }

    /// After Pass 2 analyzed the entry that starts at `entry_start`: each
    /// nested `def` a read reached before it is a file binding set before the
    /// entry, and each merged binding the entry left waiting is a file binding
    /// defined after it. Nothing reads the last entry's merged bindings.
    pub(super) fn settle_file_entry(
        &mut self,
        owner: usize,
        entry_start: usize,
        last: bool,
        bindings: &mut Vec<(Binding, Hir)>,
        lambda_entries: &mut [(usize, Binding, &Syntax)],
    ) {
        let mut frame = std::mem::take(&mut self.defs.owners[owner]);
        let hoisted = self.take_hoisted(&mut frame);
        let shift = hoisted.len();
        bindings.splice(entry_start..entry_start, hoisted);
        for (idx, _, _) in lambda_entries
            .iter_mut()
            .filter(|(idx, _, _)| *idx >= entry_start)
        {
            *idx += shift;
        }
        if !last {
            bindings.extend(self.take_merged_entries());
        }
    }
}
