// audited: 2026-11-19
//! The epoch 18 rule's letrecs over source text: each binder planned after the moves becomes a letrec or let, keeping layout and comments.
//!
//! docs/epochs.md
//!
//! A binder's body is the rest of the forms that hold it, so two binders of
//! one body may close at one byte. The plan lists an outer binder first, and
//! `apply_edits` keeps the order of insertions at one byte, which puts the
//! inner binder's close before the outer one's.

use super::defs::{binder_edits, shifted, BindingForm, Note};
use super::*;
use crate::epoch::forward::{self, body_items};
use crate::reader::read_syntax_all;
use crate::syntax::SyntaxHeap;

/// `text`, which every earlier rule has rewritten, with each binder the rule
/// plans written as a letrec or a let. Answers the new text, the number of
/// defs bound, and a note for each read or def the rule leaves. `notes` are
/// the earlier rules', pointing into `text`, and they move with the edits.
pub(crate) fn bind_forward(
    text: &str,
    name: &str,
    notes: &mut [Note],
) -> Result<(String, usize, Vec<Note>), String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    let plan = forward::plan(&forms);
    // The reader's spans start after a shebang line, and an edit indexes the
    // text whole.
    let shebang = shebang_len(text);
    let mut edits: Vec<Edit> = plan
        .binders
        .iter()
        .flat_map(|binder| {
            let items = body_items(&forms, &binder.path);
            let form = BindingForm {
                items,
                first: binder.first,
                count: binder.count,
                end: items.len(),
                head: binder.head,
                replaces: None,
            };
            binder_edits(text, shebang, &form)
        })
        .collect();
    let bound = apply_edits(text, &mut edits)?;
    for note in notes.iter_mut() {
        note.at = shifted(note.at, &edits);
    }
    let left = plan
        .left
        .iter()
        .map(|left| Note {
            at: shifted(left.span.start as usize + shebang, &edits),
            text: left.message.clone(),
        })
        .collect();
    let count = plan.binders.iter().map(|b| b.count).sum();
    Ok((bound, count, left))
}
