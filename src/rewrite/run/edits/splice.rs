// audited: 2026-11-20
//! The epoch 18 rule's splices over source text: each begin that splices loses its head and its closing paren, and its forms keep their layout.
//!
//! docs/epochs/bodies.md
//!
//! The space after the head goes with it, up to the first form. When a
//! comment stands there, only the spaces before the comment go, so the
//! comment keeps its line.

use super::defs::{shifted, Note};
use super::*;
use crate::epoch::forward;
use crate::epoch::placement::{is_begin, node_at};
use crate::reader::read_syntax_all;
use crate::syntax::{Syntax, SyntaxHeap};

/// `text`, which the epoch 17 rule has rewritten, with each begin the epoch
/// 18 rule splices spliced. Answers the new text and the number of begins
/// spliced. `notes` point into `text`, and they move with it.
pub(super) fn splice_begins(
    text: &str,
    name: &str,
    notes: &mut [Note],
) -> Result<(String, usize), String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    // The reader's spans start after a shebang line, and an edit indexes the
    // text whole.
    let shebang = shebang_len(text);
    let mut edits = Vec::new();
    let mut spliced = 0;
    for path in forward::splices(&forms) {
        spliced += begin_edits(text, shebang, node_at(&forms, &path), &mut edits);
    }
    let text = apply_edits(text, &mut edits)?;
    for note in notes.iter_mut() {
        note.at = shifted(note.at, &edits);
    }
    Ok((text, spliced))
}

/// Push the edits that splice `begin`, and each begin directly in it. Answers
/// the number of begins they splice.
fn begin_edits(text: &str, shebang: usize, begin: &Syntax, edits: &mut Vec<Edit>) -> usize {
    let at = |offset: u32| offset as usize + shebang;
    let items = begin.kind.children();
    let (start, end) = (at(begin.span.start), at(begin.span.end));
    let Some(first) = items.get(1) else {
        edits.push(Edit::replace(start, end, ""));
        return 1;
    };
    let head = at(items[0].span.end);
    let gap = &text[head..at(first.span.start)];
    let space = if gap.trim().is_empty() {
        gap.len()
    } else {
        gap.len() - gap.trim_start_matches([' ', '\t']).len()
    };
    edits.push(Edit::replace(start, head + space, ""));
    edits.push(Edit::replace(end - 1, end, ""));
    let inner: usize = items[1..]
        .iter()
        .filter(|child| is_begin(child))
        .map(|child| begin_edits(text, shebang, child, edits))
        .sum();
    1 + inner
}
