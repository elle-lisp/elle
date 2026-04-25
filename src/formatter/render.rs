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

/// Render a Doc tree to a string with the given configuration.
pub fn render(doc: &Doc, config: &FormatterConfig) -> String {
    let mut out = String::new();
    let ctx = LayoutCtx {
        indent_width: config.indent_width,
        line_width: config.line_length,
    };
    let mut last_cb = false;
    ctx.layout(doc, 0, 0, false, &mut last_cb, &mut out);
    out
}

/// Layout context carrying configuration.
struct LayoutCtx {
    indent_width: usize,
    line_width: usize,
}

impl LayoutCtx {
    /// Recursively layout a Doc.
    ///
    /// Returns the column position after laying out the doc.
    ///
    /// - `col`: current column position
    /// - `indent`: current indentation in absolute columns (not levels)
    /// - `broken`: true if we're in a broken (newline) context from an enclosing Group
    /// - `out`: output string
    fn layout(
        &self,
        doc: &Doc,
        col: usize,
        indent: usize,
        broken: bool,
        last_cb: &mut bool,
        out: &mut String,
    ) -> usize {
        match doc {
            Doc::Empty => col,

            Doc::Text(s) => {
                *last_cb = false;
                out.push_str(s);
                col + s.len()
            }

            Doc::Concat(docs) => {
                let mut current_col = col;
                for d in docs {
                    current_col = self.layout(d, current_col, indent, broken, last_cb, out);
                }
                current_col
            }

            Doc::Nest(n, inner) => self.layout(
                inner,
                col,
                indent + n * self.indent_width,
                broken,
                last_cb,
                out,
            ),

            Doc::Break => {
                if broken {
                    if *last_cb {
                        *last_cb = false;
                        col
                    } else {
                        self.emit_newline(indent, out)
                    }
                } else {
                    *last_cb = false;
                    out.push(' ');
                    col + 1
                }
            }

            Doc::Group(inner) => match measure_flat(inner) {
                Some(flat_width) if col + flat_width <= self.line_width => {
                    self.layout(inner, col, indent, false, last_cb, out)
                }
                _ => self.layout(inner, col, indent, true, last_cb, out),
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
                let new_indent = if col <= self.line_width / 2 {
                    col
                } else {
                    indent
                };
                self.layout(inner, col, new_indent, broken, last_cb, out)
            }
        }
    }

    /// Emit newline + indent spaces. Returns the new column.
    fn emit_newline(&self, indent: usize, out: &mut String) -> usize {
        out.push('\n');
        let spaces = " ".repeat(indent);
        out.push_str(&spaces);
        indent
    }
}

/// Measure the width of a doc when laid out flat (no breaks).
///
/// Returns `Some(width)` if the doc can be laid out flat, `None` if it
/// contains a HardBreak (which can never be flat).
pub(super) fn measure_flat(doc: &Doc) -> Option<usize> {
    match doc {
        Doc::Empty => Some(0),
        Doc::Text(s) => Some(s.len()),
        Doc::Concat(docs) => {
            let mut total: usize = 0;
            for d in docs {
                total = total.checked_add(measure_flat(d)?)?;
            }
            Some(total)
        }
        Doc::Nest(_, inner) => measure_flat(inner),
        Doc::Break => Some(1),
        Doc::Group(inner) => measure_flat(inner),
        Doc::HardBreak => None,
        Doc::CommentBreak => None,
        Doc::Align(inner) => measure_flat(inner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> FormatterConfig {
        FormatterConfig::default()
    }

    #[test]
    fn test_empty() {
        assert_eq!(render(&Doc::empty(), &default_config()), "");
    }

    #[test]
    fn test_text() {
        assert_eq!(render(&Doc::text("hello"), &default_config()), "hello");
    }

    #[test]
    fn test_concat_texts() {
        let doc = Doc::concat([Doc::text("hello"), Doc::text(" world")]);
        assert_eq!(render(&doc, &default_config()), "hello world");
    }

    #[test]
    fn test_group_fits() {
        let doc = Doc::concat([
            Doc::text("("),
            Doc::concat([Doc::text("a"), Doc::Break, Doc::text("b")]).group(),
            Doc::text(")"),
        ]);
        assert_eq!(render(&doc, &default_config()), "(a b)");
    }

    #[test]
    fn test_group_breaks() {
        let config = FormatterConfig::new().with_line_length(10);
        let doc = Doc::concat([
            Doc::text("("),
            Doc::concat([Doc::text("hello"), Doc::Break, Doc::text("world")])
                .nest(1)
                .group(),
            Doc::text(")"),
        ]);
        let result = render(&doc, &config);
        assert!(result.contains('\n'), "should break: got {:?}", result);
        let lines: Vec<&str> = result.lines().collect();
        assert!(
            lines[1].starts_with("  "),
            "second line should be indented: {:?}",
            lines
        );
        assert!(
            lines[1].contains("world"),
            "second line should contain 'world': {:?}",
            lines
        );
    }

    #[test]
    fn test_nest_indentation_with_leading_break() {
        let config = FormatterConfig::new()
            .with_line_length(10)
            .with_indent_width(2);
        let doc = Doc::concat([
            Doc::text("("),
            Doc::concat([
                Doc::HardBreak,
                Doc::text("a"),
                Doc::HardBreak,
                Doc::text("b"),
            ])
            .nest(1),
            Doc::HardBreak,
            Doc::text(")"),
        ]);
        let result = render(&doc, &config);
        assert_eq!(result, "(\n  a\n  b\n)", "got: {:?}", result);
    }

    #[test]
    fn test_nest_indentation_same_line() {
        let config = FormatterConfig::new()
            .with_line_length(10)
            .with_indent_width(2);
        let doc = Doc::concat([
            Doc::text("("),
            Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]).nest(1),
            Doc::HardBreak,
            Doc::text(")"),
        ]);
        let result = render(&doc, &config);
        assert_eq!(result, "(a\n  b\n)", "got: {:?}", result);
    }

    #[test]
    fn test_hardbreak_alone() {
        let doc = Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]);
        assert_eq!(render(&doc, &default_config()), "a\nb");
    }

    #[test]
    fn test_hardbreak_forces_group_break() {
        let doc = Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]).group();
        let result = render(&doc, &default_config());
        assert_eq!(result, "a\nb", "got: {:?}", result);
    }

    #[test]
    fn test_nested_group_outer_fits_inner_also_fits() {
        let config = FormatterConfig::new().with_line_length(40);
        let doc = Doc::concat([
            Doc::text("outer-start "),
            Doc::concat([Doc::text("inner-a"), Doc::Break, Doc::text("inner-b")]).group(),
        ])
        .group();
        let result = render(&doc, &config);
        assert_eq!(result, "outer-start inner-a inner-b");
    }

    #[test]
    fn test_nested_group_outer_breaks_inner_also_breaks() {
        let config = FormatterConfig::new().with_line_length(10);
        let doc = Doc::concat([
            Doc::text("start"),
            Doc::Break,
            Doc::concat([Doc::text("inner-a"), Doc::Break, Doc::text("inner-b")])
                .nest(1)
                .group(),
        ])
        .nest(1)
        .group();
        let result = render(&doc, &config);
        assert!(result.contains('\n'), "should break: got {:?}", result);
    }

    #[test]
    fn test_list_formatting_short() {
        let doc = Doc::concat([
            Doc::text("("),
            Doc::intersperse([Doc::text("a"), Doc::text("b"), Doc::text("c")]).group(),
            Doc::text(")"),
        ]);
        assert_eq!(render(&doc, &default_config()), "(a b c)");
    }

    #[test]
    fn test_list_formatting_long() {
        let config = FormatterConfig::new().with_line_length(20);
        let doc = Doc::concat([
            Doc::text("("),
            Doc::intersperse([Doc::text("long-argument-1"), Doc::text("long-argument-2")])
                .nest(1)
                .group(),
            Doc::text(")"),
        ]);
        let result = render(&doc, &config);
        assert!(result.contains('\n'), "should break: got {:?}", result);
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 2, "expected 2+ lines, got: {:?}", lines);
        assert!(
            lines[0].starts_with("(long-argument-1"),
            "first line should start with (long-argument-1: {:?}",
            lines
        );
        assert!(
            lines[1].starts_with("  long-argument-2"),
            "second line should be indented: {:?}",
            lines
        );
    }

    #[test]
    fn test_measure_flat_values() {
        assert_eq!(measure_flat(&Doc::empty()), Some(0));
        assert_eq!(measure_flat(&Doc::text("hello")), Some(5));
        assert_eq!(measure_flat(&Doc::Break), Some(1));
        assert_eq!(measure_flat(&Doc::HardBreak), None);
        assert_eq!(
            measure_flat(&Doc::concat([Doc::text("ab"), Doc::Break, Doc::text("cd")])),
            Some(5)
        );
        assert_eq!(
            measure_flat(&Doc::concat([
                Doc::text("a"),
                Doc::HardBreak,
                Doc::text("b")
            ])),
            None
        );
    }

    #[test]
    fn test_deeply_nested_indentation() {
        let config = FormatterConfig::new().with_indent_width(2);
        let doc = Doc::concat([
            Doc::text("outer"),
            Doc::concat([
                Doc::HardBreak,
                Doc::text("mid"),
                Doc::concat([Doc::HardBreak, Doc::text("inner")]).nest(1),
            ])
            .nest(1),
        ]);
        let result = render(&doc, &config);
        let expected = "outer\n  mid\n    inner";
        assert_eq!(result, expected, "got: {:?}", result);
    }

    #[test]
    fn test_nest_only_affects_breaks() {
        let config = FormatterConfig::new().with_indent_width(2);
        let doc = Doc::concat([
            Doc::text("a"),
            Doc::concat([Doc::text("b"), Doc::HardBreak, Doc::text("c")]).nest(1),
        ]);
        let result = render(&doc, &config);
        assert_eq!(result, "ab\n  c", "got: {:?}", result);
    }

    #[test]
    fn test_align() {
        let config = FormatterConfig::new().with_line_length(15);
        let doc = Doc::concat([
            Doc::text("(foo "),
            Doc::align(
                Doc::concat([
                    Doc::text("long-a"),
                    Doc::Break,
                    Doc::text("long-b"),
                    Doc::Break,
                    Doc::text("long-c"),
                ])
                .group(),
            ),
            Doc::text(")"),
        ]);
        let result = render(&doc, &config);
        // When broken, long-b and long-c align to column 5 (after "(foo ")
        assert_eq!(
            result, "(foo long-a\n     long-b\n     long-c)",
            "got: {:?}",
            result
        );
    }
}
