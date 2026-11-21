// audited: 2026-11-20
//! The epoch 16 fold over source text: an old module's forms move into its last fn form, by edits that keep the layout and the comments.
//!
//! docs/epochs.md
//!
//! The edits touch the function's head and the ends of the forms around it,
//! and never a form's own text. They leave the body's indent as it was, and
//! `elle fmt` re-indents it.

use super::*;
use crate::epoch::epoch_declaration;
use crate::epoch::fold::{self, Shape};
use crate::reader::read_syntax_all;
use crate::syntax::SyntaxHeap;

/// `text`, which the table rules have rewritten, with its forms folded into
/// its last `fn` form when the plan answers a fold. Answers the new text and
/// the number of rewrites: one or none.
pub(crate) fn fold_module(text: &str, name: &str) -> Result<(String, usize), String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    // The epoch tag the rewrite wrote is no form of the module.
    let tagged = forms.first().and_then(epoch_declaration).is_some();
    let Some(fold) = fold::plan(&forms[usize::from(tagged)..]) else {
        return Ok((text.to_string(), 0));
    };
    // The reader's spans start after a shebang line, and an edit indexes the
    // text whole.
    let shebang = shebang_len(text);
    let at = |offset: u32| offset as usize + shebang;
    let insert = |byte_offset: usize, replacement: String| Edit {
        byte_offset,
        byte_len: 0,
        replacement,
    };
    let first = at(fold.lead[0].span.start);
    let start = at(fold.function.span.start);
    let mut edits = match &fold.shape {
        Shape::Body => {
            let mut head = text[start..at(fold.items[1].span.end)].to_string();
            if fold.doc {
                let doc = fold.items[2].span;
                head = format!("{head} {}", &text[at(doc.start)..at(doc.end)]);
            }
            let body = at(fold.items[if fold.doc { 3 } else { 2 }].span.start);
            vec![
                insert(first, format!("{head} ")),
                Edit {
                    byte_offset: start,
                    byte_len: body - start,
                    replacement: String::new(),
                },
            ]
        }
        Shape::Apply { name } => vec![
            insert(first, format!("(fn [& {name}] ")),
            insert(start, "(apply ".to_string()),
            insert(at(fold.function.span.end), format!(" {name}))")),
        ],
    };
    Ok((apply_edits(text, &mut edits)?, 1))
}
