// audited: 2026-11-20
//! The epoch 18 moves over source text: each def that moves is cut from its place and written above its new successor, with comments.
//!
//! docs/epochs/bodies.md
//!
//! The begins that splice go first, and the text is read again. The text then
//! moves one body at a time, a body inside another first, and is read again
//! before each, so a def that moves carries the moves made inside it. A def
//! on lines of its own takes the comment lines directly above it and a blank
//! line after it. A def that shares its line moves alone.

use super::defs::{shifted, Note};
use super::splice::splice_begins;
use super::*;
use crate::epoch::forward::{self, body_items, Reorder};
use crate::reader::read_syntax_all;
use crate::syntax::{Syntax, SyntaxHeap};

/// `text`, which the epoch 17 rule has rewritten, with each begin the epoch 18
/// rule splices spliced, and each def it moves above the first form that
/// reads it. Answers the new text and the number of begins spliced and defs
/// moved. `notes` point into `text`, and they move with it.
pub(crate) fn move_defs(
    text: &str,
    name: &str,
    notes: &mut [Note],
) -> Result<(String, usize), String> {
    let (mut text, spliced) = splice_begins(text, name, notes)?;
    let reorders = {
        let mut home = SyntaxHeap::new();
        forward::moves(&read_syntax_all(home.arena(), &text, name)?)
    };
    let shebang = shebang_len(&text);
    for reorder in &reorders {
        let mut home = SyntaxHeap::new();
        let forms = read_syntax_all(home.arena(), &text, name)?;
        let moves = moves(&text, shebang, body_items(&forms, &reorder.path), reorder);
        let mut edits = edits(&moves);
        for note in notes.iter_mut() {
            note.at = moved_at(note.at, &moves, &edits);
        }
        text = apply_edits(&text, &mut edits)?;
    }
    let moved: usize = reorders.iter().map(|r| r.moved.len()).sum();
    Ok((text, spliced + moved))
}

/// Where one form's text lies.
struct Block {
    /// The form's own bytes.
    start: usize,
    end: usize,
    /// Whether only space stands before the form on its line.
    starts_line: bool,
    /// Whether the form stands on lines of its own, beside at most a comment
    /// that ends its last line.
    lines: bool,
    /// The start of the comment lines directly above the form, or of its own
    /// line.
    top: usize,
    /// Past the form's last line, and past a blank line after it.
    bottom: usize,
    /// The form's column, counted from 0.
    col: usize,
}

/// One def that moves.
struct Move {
    /// The bytes cut from where the def stood.
    cut: (usize, usize),
    /// Where its text is written.
    at: usize,
    /// The bytes of `text` written there.
    from: usize,
    /// What is written: those bytes and what parts them from the next form.
    written: String,
    /// Where the written text starts among all written at `at`.
    offset: usize,
}

/// The moves `reorder` makes among `items`, the forms of its body's parent.
fn moves(text: &str, shebang: usize, items: &[Syntax], reorder: &Reorder) -> Vec<Move> {
    let block = |k: usize| Block::of(text, shebang, &items[k]);
    let mut moves: Vec<Move> = Vec::new();
    for (pos, &k) in reorder.order.iter().enumerate() {
        if !reorder.moved.contains(&k) {
            continue;
        }
        let anchor = reorder.order[pos + 1..]
            .iter()
            .find(|g| !reorder.moved.contains(g))
            .map(|&g| block(g))
            .expect("the body's last form never moves");
        let m = block(k);
        let (at, from, written) = if m.lines && anchor.starts_line {
            (anchor.top, m.top, text[m.top..m.bottom].to_string())
        } else if m.lines {
            let lines = text[m.top..m.bottom].trim();
            let from = m.top + text[m.top..].len() - text[m.top..].trim_start().len();
            (
                anchor.start,
                from,
                format!("{lines}\n{}", " ".repeat(anchor.col)),
            )
        } else {
            (anchor.start, m.start, format!("{} ", &text[m.start..m.end]))
        };
        let offset = moves
            .iter()
            .filter(|earlier| earlier.at == at)
            .map(|earlier| earlier.written.len())
            .sum();
        moves.push(Move {
            cut: m.cut(text),
            at,
            from,
            written,
            offset,
        });
    }
    moves
}

/// The edits that make `moves`: a cut for each, and one insertion at each
/// place that receives one, which holds them in their order.
fn edits(moves: &[Move]) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    for m in moves {
        edits.push(Edit::replace(m.cut.0, m.cut.1, ""));
        match edits
            .iter_mut()
            .find(|e| e.byte_len == 0 && e.byte_offset == m.at)
        {
            Some(insertion) => insertion.replacement.push_str(&m.written),
            None => edits.push(Edit::insert(m.at, &m.written)),
        }
    }
    edits
}

/// Where the byte at `at` stands once `edits` make `moves`: in the text of the
/// def it moves with, or shifted by the edits.
fn moved_at(at: usize, moves: &[Move], edits: &[Edit]) -> usize {
    match moves.iter().find(|m| m.cut.0 <= at && at < m.cut.1) {
        Some(m) => {
            let within = at.saturating_sub(m.from).min(m.written.len());
            shifted(m.at, edits) + m.offset + within
        }
        None => shifted(at, edits),
    }
}

impl Block {
    fn of(text: &str, shebang: usize, node: &Syntax) -> Block {
        let start = node.span.start as usize + shebang;
        let end = node.span.end as usize + shebang;
        let line = line_start(text, start);
        let starts_line = text[line..start].trim().is_empty();
        let rest = text[end..line_end(text, end)].trim_start();
        let lines = starts_line && (rest.is_empty() || rest.starts_with('#'));
        let mut top = line;
        while top > shebang {
            let above = line_start(text, top - 1);
            if above < shebang || !text[above..top - 1].trim_start().starts_with('#') {
                break;
            }
            top = above;
        }
        let mut bottom = (line_end(text, end) + 1).min(text.len());
        let next = line_end(text, bottom);
        if next < text.len() && text[bottom..next].trim().is_empty() {
            bottom = next + 1;
        }
        Block {
            start,
            end,
            starts_line,
            lines,
            top,
            bottom,
            col: start - line,
        }
    }

    /// The bytes a move cuts: the block's lines, or the form and the space
    /// that parts it from its neighbor on its line.
    fn cut(&self, text: &str) -> (usize, usize) {
        let space = |c: char| c == ' ' || c == '\t';
        if self.lines {
            (self.top, self.bottom)
        } else if self.starts_line {
            let after = text[self.end..].len() - text[self.end..].trim_start_matches(space).len();
            (self.start, self.end + after)
        } else {
            let before =
                text[..self.start].len() - text[..self.start].trim_end_matches(space).len();
            (self.start - before, self.end)
        }
    }
}

/// The start of the line that holds the byte at `at`.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line that holds the byte at `at`: its newline, or the end
/// of the text.
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}
