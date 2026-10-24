// audited: 2026-11-13
//! A file's spliced top-level `begin`, and the `def` forms nested in its forms.
//!
//! docs/bindings.md
//! docs/pipeline.md

use super::*;
use crate::hir::analyze::owned::reach;

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
    ) {
        let mut frame = std::mem::take(&mut self.defs.owners[owner]);
        let hoisted = self.take_hoisted(&mut frame);
        bindings.splice(entry_start..entry_start, hoisted);
        if !last {
            bindings.extend(self.take_merged_entries());
        }
    }
}
