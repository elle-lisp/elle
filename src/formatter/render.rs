//! Doc renderer — evaluates a Doc tree within a line-width budget.
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
//! to the current column.

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

/// Render a Doc tree to a string with the given configuration.
pub fn render(doc: &Doc, config: &FormatterConfig) -> String {
    let mut out = String::new();
    let ctx = LayoutCtx {
        indent_width: IndentWidth::new(config.indent_width),
        line_width: LineWidth::new(config.line_length),
    };
    let mut last_cb = false;
    ctx.layout(
        doc,
        Column::ZERO,
        Indent::ZERO,
        Mode::Flat,
        &mut last_cb,
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
        last_cb: &mut bool,
        out: &mut String,
    ) -> Column {
        match doc {
            Doc::Empty => col,

            Doc::Text(s) => {
                *last_cb = false;
                out.push_str(s);
                col.advance(s.len())
            }

            Doc::Concat(docs) => {
                let mut current_col = col;
                for d in docs {
                    current_col = self.layout(d, current_col, indent, mode, last_cb, out);
                }
                current_col
            }

            Doc::Nest(n, inner) => {
                let indent = indent.plus(IndentLevel::new(*n).widen(self.indent_width));
                self.layout(inner, col, indent, mode, last_cb, out)
            }

            Doc::Break => match mode {
                Mode::Broken => {
                    if *last_cb {
                        *last_cb = false;
                        col
                    } else {
                        self.emit_newline(indent, out)
                    }
                }
                Mode::Flat => {
                    *last_cb = false;
                    out.push(' ');
                    col.advance(1)
                }
            },

            Doc::Group(inner) => match measure_flat(inner) {
                Some(flat_width) if col.plus(flat_width).fits(self.line_width) => {
                    self.layout(inner, col, indent, Mode::Flat, last_cb, out)
                }
                _ => self.layout(inner, col, indent, Mode::Broken, last_cb, out),
            },

            Doc::HardBreak => {
                if *last_cb {
                    *last_cb = false;
                    col
                } else {
                    self.emit_newline(indent, out)
                }
            }

            Doc::CommentBreak => {
                if *last_cb {
                    col
                } else {
                    *last_cb = true;
                    self.emit_newline(indent, out)
                }
            }

            Doc::Align(inner) => {
                // Cap alignment: if we're past half the line width, don't
                // create a new alignment point — keep the enclosing indent.
                // This prevents cascading Aligns from pushing deeply nested
                // code off the right edge.
                let new_indent = if col <= self.line_width.half() {
                    col.as_indent()
                } else {
                    indent
                };
                self.layout(inner, col, new_indent, mode, last_cb, out)
            }
        }
    }

    /// Emit newline + indent spaces. Returns the new column.
    fn emit_newline(&self, indent: Indent, out: &mut String) -> Column {
        out.push('\n');
        out.push_str(&indent.spaces());
        indent.as_column()
    }
}

/// Measure the width of a doc when laid out flat (no breaks).
///
/// Returns `Some(width)` if the doc can be laid out flat, `None` if it
/// contains a HardBreak (which can never be flat).
pub(super) fn measure_flat(doc: &Doc) -> Option<Column> {
    match doc {
        Doc::Empty => Some(Column::ZERO),
        Doc::Text(s) => Some(Column::new(s.len())),
        Doc::Concat(docs) => {
            let mut total = Column::ZERO;
            for d in docs {
                total = total.checked_plus(measure_flat(d)?)?;
            }
            Some(total)
        }
        Doc::Nest(_, inner) => measure_flat(inner),
        Doc::Break => Some(Column::new(1)),
        Doc::Group(inner) => measure_flat(inner),
        Doc::HardBreak => None,
        Doc::CommentBreak => None,
        Doc::Align(inner) => measure_flat(inner),
    }
}

#[cfg(test)]
mod tests;
