// audited: 2026-11-20
//! The epoch 17 rule's assigns over source text: each def that assigns becomes an assign, and each binding it assigns goes before its home.
//!
//! docs/epochs/bodies.md
//!
//! The edits touch a def's head, the `@` of its name, and for a `defn` the
//! ends of its function, and never a value. A binding takes a line of its own
//! when its home starts a line, and shares the home's line otherwise.

use super::*;
use crate::epoch::placement::{self, node_at, Def, Kind};
use crate::reader::read_syntax_all;
use crate::syntax::SyntaxHeap;

/// `text`, which every other rule has rewritten, with each def the epoch 17
/// rule's first walk names written as an assign. Answers the new text and
/// the number of defs that assign.
pub(super) fn assign_defs(text: &str, name: &str) -> Result<(String, usize), String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    let assigns = placement::assigns(&forms);
    // The reader's spans start after a shebang line, and an edit indexes the
    // text whole.
    let shebang = shebang_len(text);
    let at = |offset: u32| offset as usize + shebang;
    let mut edits = Vec::new();
    for path in &assigns.defs {
        let def = Def::of(node_at(&forms, path)).expect("an assign names a def");
        let head = def.items[0].span;
        edits.push(Edit::replace(at(head.start), at(head.end), "assign"));
        let name = def.name();
        if name.as_symbol().is_some_and(|n| n.starts_with('@')) {
            let start = at(name.span.start);
            edits.push(Edit::replace(start, start + 1, ""));
        }
        // The defn's own paren closes the function.
        if def.kind == Kind::Defn {
            edits.push(Edit::insert(at(def.items[2].span.start), "(fn "));
            edits.push(Edit::insert(at(def.node.span.end), ")"));
        }
    }
    for decl in &assigns.decls {
        let start = at(node_at(&forms, &decl.home).span.start);
        let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent = &text[line..start];
        let written = if indent.trim().is_empty() {
            format!("{}\n{indent}", decl.text())
        } else {
            format!("{} ", decl.text())
        };
        edits.push(Edit::insert(start, &written));
    }
    Ok((apply_edits(text, &mut edits)?, assigns.defs.len()))
}
