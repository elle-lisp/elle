// audited: 2026-11-17
//! A file's top-level `begin`, whose forms stand at top level.
//!
//! docs/bindings.md

use super::*;

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
