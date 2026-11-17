// audited: 2026-11-17
//! The epoch 17 rule over source text: each run of defs the plan moves becomes a let, by edits that keep the layout and the comments.
//!
//! docs/epochs.md
//!
//! The edits touch a def's head, its closing paren, and the end of the let's
//! last form, and never a value or a form in the body. So a run nested in
//! another's value or body makes edits of its own that cannot overlap.

use super::*;
use crate::epoch::placement::{self, node_at, Def, Kind, Placement};
use crate::reader::read_syntax_all;
use crate::syntax::{Syntax, SyntaxHeap};

/// `text`, which every other rule has rewritten, with each run of defs the
/// rule moves placed in a let. Answers the new text, the number of defs that
/// moved, and a note `LINE:COL: why` for each def the rule leaves, positioned
/// in the new text.
pub(crate) fn place_defs(text: &str, name: &str) -> Result<(String, usize, Vec<String>), String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    let plan = placement::plan(&forms);
    // The reader's spans start after a shebang line, and an edit indexes the
    // text whole.
    let shebang = shebang_len(text);
    let mut edits: Vec<Edit> = plan
        .placements
        .iter()
        .flat_map(|p| placement_edits(text, shebang, &forms, p))
        .collect();
    let placed = apply_edits(text, &mut edits)?;
    let notes = plan
        .notes
        .iter()
        .map(|note| {
            let at = shifted(note.span.start as usize + shebang, &edits);
            let (line, col) = line_col(&placed, at);
            format!(
                "{line}:{col}: the epoch 17 rule leaves this def: {}",
                note.reason
            )
        })
        .collect();
    let moved = plan.placements.iter().map(|p| p.count).sum();
    Ok((placed, moved, notes))
}

/// The edits one placement makes.
fn placement_edits(text: &str, shebang: usize, forms: &[Syntax], p: &Placement) -> Vec<Edit> {
    let at = |offset: u32| offset as usize + shebang;
    let edit = |from: usize, to: usize, replacement: &str| Edit {
        byte_offset: from,
        byte_len: to - from,
        replacement: replacement.to_string(),
    };
    let blank = |from: u32, to: u32| text[at(from)..at(to)].trim().is_empty();
    let parent = node_at(forms, &p.path);
    let items = parent.kind.children();
    let defs: Vec<Def<'_>> = items[p.first..p.first + p.count]
        .iter()
        .filter_map(Def::of)
        .collect();
    let body = &items[p.first + p.count..p.end];
    // The let takes the begin's place only where nothing but space stands
    // between the begin's head and its first def. A comment there would go
    // with the head, so the begin stays around the let.
    let whole = p.replaces && blank(items[0].span.end, defs[0].node.span.start);
    let mut edits = Vec::new();
    for (k, def) in defs.iter().enumerate() {
        let head = &def.items[0];
        let name = def.name();
        let start = if k == 0 && whole {
            parent.span.start
        } else {
            def.node.span.start
        };
        let open = if k == 0 { "(let [" } else { "" };
        edits.push(edit(at(start), at(head.span.end), open));
        if blank(head.span.end, name.span.start) {
            edits.push(edit(at(head.span.end), at(name.span.start), ""));
        }
        if def.needs_at() {
            edits.push(edit(at(name.span.start), at(name.span.start), "@"));
        }
        let last = k + 1 == defs.len();
        let close = match (last, body.is_empty()) {
            (false, _) => String::new(),
            (true, false) => "]".to_string(),
            (true, true) => format!("] {}{}", def.plain_name(), if whole { "" } else { ")" }),
        };
        let end = at(def.node.span.end);
        match def.kind {
            // The defn's own paren closes the function.
            Kind::Defn => {
                let params = at(def.items[2].span.start);
                edits.push(edit(params, params, "(fn "));
                if last {
                    edits.push(edit(end, end, &close));
                }
            }
            Kind::Def | Kind::Var => edits.push(edit(end - 1, end, &close)),
        }
    }
    match body.last() {
        Some(form) if !whole => {
            let end = at(form.span.end);
            edits.push(edit(end, end, ")"));
        }
        _ => {}
    }
    edits
}

/// Where the byte at `at` stands once `edits` apply.
fn shifted(at: usize, edits: &[Edit]) -> usize {
    let before =
        |e: &&Edit| e.byte_offset + e.byte_len <= at && (e.byte_len > 0 || e.byte_offset < at);
    edits.iter().filter(before).fold(at as isize, |at, e| {
        at + e.replacement.len() as isize - e.byte_len as isize
    }) as usize
}

/// The line and column of the byte at `at`, counted from 1 as the reader
/// counts them: a column in characters.
fn line_col(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, col)
}
