// audited: 2026-10-27
//! Doc renderer — evaluates a Doc tree within a line-width budget.
//!
//! src/formatter/README.md
//!
//! Implements a simplified version of Wadler's `best(w, k, doc)` algorithm.
//! At each `Group`, tries flat layout first; if it exceeds page width,
//! falls back to broken layout (Breaks become newlines with indentation).
//!
//! The renderer tracks the current column position as a running counter
//! (O(1) per step) rather than re-scanning the output string.
//!
//! Indent is tracked in absolute columns (number of spaces), not indent
//! levels. Nest(n) adds n * indent_width to the indent; Align sets indent
//! to the current column; Hang chooses between the two by the column.

use super::config::FormatterConfig;
use super::doc::Doc;
use super::metrics::{Column, Indent, IndentLevel, IndentWidth, LineWidth};

/// Whether the current context lays content out flat (spaces) or broken
/// (newlines + indent). Replaces a bare `broken: bool` so it can't be
/// confused with the other boolean state the renderer threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Broken,
}

/// What the renderer has written since the last newline. Named fields, so the
/// two flags cannot trade places at a call site.
struct Line {
    /// A CommentBreak ended the line, and no separator has answered it yet.
    after_comment: bool,
    /// Nothing but indent since the last newline, or since the document start.
    at_start: bool,
}

/// Render a Doc tree to a string with the given configuration.
pub fn render(doc: &Doc, config: &FormatterConfig) -> String {
    let mut out = String::new();
    let ctx = LayoutCtx {
        indent_width: IndentWidth::new(config.indent_width),
        line_width: LineWidth::new(config.line_length),
    };
    let mut line = Line {
        after_comment: false,
        at_start: true,
    };
    ctx.layout(
        doc,
        Column::ZERO,
        Indent::ZERO,
        Mode::Flat,
        &mut line,
        &mut out,
    );
    out
}

/// Layout context carrying configuration.
struct LayoutCtx {
    indent_width: IndentWidth,
    line_width: LineWidth,
}

impl LayoutCtx {
    /// Recursively layout a Doc.
    ///
    /// Returns the column position after laying out the doc.
    ///
    /// - `col`: current column position
    /// - `indent`: current indentation in absolute columns (not levels)
    /// - `mode`: whether an enclosing Group chose flat or broken layout
    /// - `out`: output string
    fn layout(
        &self,
        doc: &Doc,
        col: Column,
        indent: Indent,
        mode: Mode,
        line: &mut Line,
        out: &mut String,
    ) -> Column {
        match doc {
            Doc::Empty => col,

            Doc::Text(s) => {
                if !s.is_empty() {
                    line.after_comment = false;
                    line.at_start = false;
                }
                out.push_str(s);
                col.advance(s.len())
            }

            Doc::Concat(docs) => {
                let mut current_col = col;
                for d in docs {
                    current_col = self.layout(d, current_col, indent, mode, line, out);
                }
                current_col
            }

            Doc::Nest(n, inner) => {
                let indent = indent.plus(IndentLevel::new(*n).widen(self.indent_width));
                self.layout(inner, col, indent, mode, line, out)
            }

            Doc::Break if mode == Mode::Broken => {
                self.newline_unless_absorbed(indent, col, line, out)
            }

            Doc::Break | Doc::Space => {
                if line.after_comment {
                    line.after_comment = false;
                    col
                } else {
                    line.at_start = false;
                    out.push(' ');
                    col.advance(1)
                }
            }

            Doc::Group(inner) => match measure_flat(inner) {
                Some(flat_width) if col.plus(flat_width).fits(self.line_width) => {
                    self.layout(inner, col, indent, Mode::Flat, line, out)
                }
                _ => self.layout(inner, col, indent, Mode::Broken, line, out),
            },

            Doc::HardBreak => self.newline_unless_absorbed(indent, col, line, out),

            Doc::CommentBreak => {
                if line.after_comment {
                    col
                } else {
                    line.after_comment = true;
                    self.emit_newline(indent, line, out)
                }
            }

            Doc::FreshLine if !line.at_start => self.emit_newline(indent, line, out),

            Doc::BlankLine if line.at_start => self.emit_newline(indent, line, out),

            Doc::FreshLine | Doc::BlankLine => col,

            Doc::Align(inner) => self.layout(inner, col, col.as_indent(), mode, line, out),

            // Near the left edge, and on a line a comment has just ended, a
            // space joins the first argument to the head and the arguments
            // align there. Past half the line width the indent stays at the
            // call's, and the first argument joins the head only if it fits.
            Doc::Hang(first, rest) => {
                let start = col.advance(1);
                let aligned = line.after_comment || start <= self.line_width.half();
                let joined = aligned
                    || measure_flat(first).is_some_and(|w| start.plus(w).fits(self.line_width));
                let col = if joined {
                    self.layout(&Doc::Space, col, indent, mode, line, out)
                } else {
                    self.emit_newline(indent, line, out)
                };
                let indent = if aligned { col.as_indent() } else { indent };
                let col = self.layout(first, col, indent, mode, line, out);
                self.layout(rest, col, indent, mode, line, out)
            }
        }
    }

    /// Emit a newline, unless a CommentBreak just did.
    fn newline_unless_absorbed(
        &self,
        indent: Indent,
        col: Column,
        line: &mut Line,
        out: &mut String,
    ) -> Column {
        if line.after_comment {
            line.after_comment = false;
            col
        } else {
            self.emit_newline(indent, line, out)
        }
    }

    /// Emit newline + indent spaces. Returns the new column.
    fn emit_newline(&self, indent: Indent, line: &mut Line, out: &mut String) -> Column {
        line.at_start = true;
        out.push('\n');
        out.push_str(&indent.spaces());
        indent.as_column()
    }
}

/// Measure the width of a doc when laid out flat (no breaks).
///
/// Returns `Some(width)` if the doc can be laid out flat, `None` if it
/// holds a HardBreak, a CommentBreak or a FreshLine, none of which is ever
/// flat.
pub(super) fn measure_flat(doc: &Doc) -> Option<Column> {
    match doc {
        Doc::Empty | Doc::BlankLine => Some(Column::ZERO),
        Doc::Text(s) => Some(Column::new(s.len())),
        Doc::Concat(docs) => {
            let mut total = Column::ZERO;
            for d in docs {
                total = total.checked_plus(measure_flat(d)?)?;
            }
            Some(total)
        }
        Doc::Nest(_, inner) => measure_flat(inner),
        Doc::Break | Doc::Space => Some(Column::new(1)),
        Doc::Group(inner) => measure_flat(inner),
        Doc::HardBreak | Doc::CommentBreak | Doc::FreshLine => None,
        Doc::Align(inner) => measure_flat(inner),
        Doc::Hang(first, rest) => Column::new(1)
            .checked_plus(measure_flat(first)?)?
            .checked_plus(measure_flat(rest)?),
    }
}

#[cfg(test)]
mod tests;
