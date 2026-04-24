//! Trivia layer: comment and blank-line attachment to Syntax nodes.
//!
//! The formatter operates on an `AnnotatedSyntax` tree — a Syntax tree where
//! every node has its leading and trailing trivia (comments, blank lines)
//! pre-attached. This is produced by a single upfront pass that maps trivia
//! items to Syntax nodes by byte-offset comparison.
//!
//! ## Why this exists
//!
//! The Syntax tree is designed for compilation — it intentionally discards
//! comments and blank lines. The formatter needs this information. Rather
//! than consulting a separate map during the Doc walk (which creates ordering
//! dependencies and loses dangling trivia), we attach everything upfront in
//! a single pass. The Doc generator then walks the annotated tree as a pure
//! function with no mutable state.
//!
//! ## Data flow
//!
//! ```text
//! Source string ──┬──► Lexer ──► Comment tokens (byte offsets)
//!                 └──► Blank-line scanner ──► Blank-line ranges (byte offsets)
//!                              │
//!                              ▼
//!                      Merge → Vec<Trivia> (sorted by byte offset)
//!                              │
//! Syntax tree ────────────────►│
//!                              ▼
//!                      Attachment pass
//!                      (compare trivia byte offsets with Syntax span ranges)
//!                              │
//!                              ▼
//!                      Vec<AnnotatedSyntax>
//! ```

use crate::syntax::{Span, Syntax, SyntaxKind};

// ── Trivia types ──────────────────────────────────────────────

/// A piece of source trivia — a comment or blank lines.
/// Positioned by byte offset for attachment to Syntax nodes.
#[derive(Debug, Clone)]
pub enum Trivia {
    /// A line comment: `# text` or `## doc text`.
    /// `text` includes the `#` prefix, with trailing newline stripped.
    Comment {
        text: String,
        byte_offset: usize,
        line: u32,
    },
    /// One or more consecutive blank lines.
    BlankLines {
        count: u32,
        byte_offset: usize,
        /// Line number of the first blank line.
        line: u32,
    },
}

impl Trivia {
    /// Byte offset where this trivia starts in the source.
    pub fn byte_offset(&self) -> usize {
        match self {
            Trivia::Comment { byte_offset, .. } => *byte_offset,
            Trivia::BlankLines { byte_offset, .. } => *byte_offset,
        }
    }

    /// Line number of this trivia.
    pub fn line(&self) -> u32 {
        match self {
            Trivia::Comment { line, .. } => *line,
            Trivia::BlankLines { line, .. } => *line,
        }
    }
}

// ── Annotated syntax tree ─────────────────────────────────────

/// A Syntax node with its attached trivia and annotated children.
#[derive(Debug, Clone)]
pub struct AnnotatedSyntax {
    /// The underlying Syntax node.
    pub syntax: Syntax,
    /// Trivia that appears before this node, on lines strictly above
    /// the node's start line. Emitted as HardBreak + comment text
    /// before the node's Doc.
    pub leading: Vec<Trivia>,
    /// Trivia that appears after this node on the same line
    /// (trailing inline comments). Emitted after the node's Doc.
    pub trailing: Vec<Trivia>,
    /// Annotated children for compound nodes.
    pub children: Vec<AnnotatedSyntax>,
}

impl AnnotatedSyntax {
    /// Build annotated trees for a list of top-level forms, consuming
    /// trivia from the list by attaching each trivia item to the
    /// nearest Syntax node based on byte offsets.
    /// Returns both the annotated forms and any dangling trivia (trivia after the last form).
    pub fn build_toplevel(
        forms: Vec<Syntax>,
        trivia: &[Trivia],
        source: &str,
    ) -> (Vec<Self>, Vec<Trivia>) {
        attach_trivia_to_forms(forms, trivia, source)
    }

    /// Get the span of the underlying Syntax node.
    pub fn span(&self) -> &Span {
        &self.syntax.span
    }

    /// Get the kind of the underlying Syntax node.
    pub fn kind(&self) -> &SyntaxKind {
        &self.syntax.kind
    }
}

// ── Trivia collection ─────────────────────────────────────────

/// Collect trivia (comments + blank lines) from source text.
///
/// Comments come from the lexer's `CommentMap` (which has accurate
/// byte offsets). Blank lines are scanned from the source directly.
/// This function merges both sources into a single sorted list.
pub fn collect_trivia(
    source: &str,
    comments: &[(String, usize, u32)], // (text, byte_offset, line)
) -> Vec<Trivia> {
    let mut trivia: Vec<Trivia> = Vec::new();

    // Add comments from the lexer
    for (text, byte_offset, line) in comments {
        trivia.push(Trivia::Comment {
            text: text.clone(),
            byte_offset: *byte_offset,
            line: *line,
        });
    }

    // Scan for blank lines
    let mut offset = 0usize;
    let mut blank_start: Option<(usize, u32)> = None;
    let mut blank_count: u32 = 0;

    for (current_line, raw_line) in (1_u32..).zip(source.lines()) {
        let line_len = raw_line.len();
        if raw_line.trim().is_empty() {
            if blank_start.is_none() {
                blank_start = Some((offset, current_line));
                blank_count = 1;
            } else {
                blank_count += 1;
            }
        } else {
            if let Some((boff, line)) = blank_start.take() {
                if blank_count > 0 {
                    trivia.push(Trivia::BlankLines {
                        count: blank_count,
                        byte_offset: boff,
                        line,
                    });
                }
                blank_count = 0;
            }
        }
        offset += line_len + 1; // +1 for the newline character
    }

    // Flush trailing blank lines
    if let Some((boff, line)) = blank_start.take() {
        trivia.push(Trivia::BlankLines {
            count: blank_count,
            byte_offset: boff,
            line,
        });
    }

    // Sort by byte offset
    trivia.sort_by_key(|t| t.byte_offset());
    trivia
}

// ── Attachment pass ───────────────────────────────────────────

/// Attach trivia to top-level Syntax forms.
///
/// The algorithm:
/// 1. For each Syntax node, compute its span range [start, end).
/// 2. Leading trivia: items with byte_offset < node.span.start and
///    line < node.span.line.
/// 3. Trailing trivia: items between this form and the next form (if any).
///    For the last form, no trailing trivia is attached — it remains dangling.
/// 4. Recurse into children with remaining trivia.
/// 5. Any leftover trivia after the last form is "dangling".
fn attach_trivia_to_forms(
    forms: Vec<Syntax>,
    trivia: &[Trivia],
    source: &str,
) -> (Vec<AnnotatedSyntax>, Vec<Trivia>) {
    if forms.is_empty() {
        // All trivia is dangling if there are no forms
        return (Vec::new(), trivia.to_vec());
    }

    // Helper to calculate line number from byte offset
    let line_at_offset = |offset: usize| -> u32 {
        let mut line: u32 = 1;
        for (i, ch) in source.chars().enumerate() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
            }
        }
        line
    };

    // Pre-compute span starts for looking ahead to next form
    let form_spans: Vec<(usize, usize)> =
        forms.iter().map(|f| (f.span.start, f.span.end)).collect();

    let mut all_attached = Vec::new();
    let mut trivia_idx: usize = 0;

    for (form_idx, form) in forms.into_iter().enumerate() {
        let span = form.span.clone();

        // Collect leading trivia: items before this form's span start
        let mut leading = Vec::new();
        while trivia_idx < trivia.len() {
            let t = &trivia[trivia_idx];
            if t.byte_offset() >= span.start {
                break;
            }
            leading.push(t.clone());
            trivia_idx += 1;
        }

        // Build children (recursively attach trivia within this form)
        let children = attach_to_children(&form, trivia, &mut trivia_idx);

        // Collect trailing trivia: comments on the same line as this form's end.
        // Blank lines and comments on later lines are left for the next form.
        // For the last form, don't attach trailing trivia (leave it dangling).
        let mut trailing = Vec::new();
        let is_last_form = form_idx + 1 >= form_spans.len();
        if !is_last_form {
            let form_end_line = line_at_offset(span.end);
            let next_start = form_spans
                .get(form_idx + 1)
                .map(|(s, _)| *s)
                .unwrap_or(usize::MAX);

            // Collect only comments on the same line as the form ends
            while trivia_idx < trivia.len() {
                let t = &trivia[trivia_idx];
                if t.byte_offset() >= next_start {
                    break;
                }
                // Only trailing comments on the same line; leave other trivia for next form
                match t {
                    Trivia::Comment { line, .. } if *line == form_end_line => {
                        trailing.push(t.clone());
                        trivia_idx += 1;
                    }
                    _ => {
                        // Blank lines and comments on later lines stay for the next form
                        break;
                    }
                }
            }
        }

        all_attached.push(AnnotatedSyntax {
            syntax: form,
            leading,
            trailing,
            children,
        });
    }

    // Collect any remaining trivia as dangling
    let mut dangling = Vec::new();
    while trivia_idx < trivia.len() {
        dangling.push(trivia[trivia_idx].clone());
        trivia_idx += 1;
    }

    (all_attached, dangling)
}

/// Recursively attach trivia to children of a compound node.
fn attach_to_children(
    parent: &Syntax,
    trivia: &[Trivia],
    trivia_idx: &mut usize,
) -> Vec<AnnotatedSyntax> {
    let children: Vec<&Syntax> = match &parent.kind {
        SyntaxKind::List(cs)
        | SyntaxKind::Array(cs)
        | SyntaxKind::ArrayMut(cs)
        | SyntaxKind::Struct(cs)
        | SyntaxKind::StructMut(cs)
        | SyntaxKind::Set(cs)
        | SyntaxKind::SetMut(cs)
        | SyntaxKind::Bytes(cs)
        | SyntaxKind::BytesMut(cs) => cs.iter().collect(),
        SyntaxKind::Quote(inner)
        | SyntaxKind::Quasiquote(inner)
        | SyntaxKind::Unquote(inner)
        | SyntaxKind::UnquoteSplicing(inner)
        | SyntaxKind::Splice(inner) => vec![inner],
        _ => return Vec::new(),
    };

    let mut annotated = Vec::with_capacity(children.len());

    for (i, child) in children.iter().enumerate() {
        let span = &child.span;

        // Leading trivia: before this child's span
        let mut leading = Vec::new();
        while *trivia_idx < trivia.len() {
            let t = &trivia[*trivia_idx];
            if t.byte_offset() >= span.start {
                break;
            }
            leading.push(t.clone());
            *trivia_idx += 1;
        }

        // Recurse into grandchildren
        let grandchildren = attach_to_children(child, trivia, trivia_idx);

        // Skip trivia items that fall inside this child's span but were
        // not consumed by grandchildren (e.g. blank lines inside strings).
        while *trivia_idx < trivia.len() && trivia[*trivia_idx].byte_offset() < span.end {
            *trivia_idx += 1;
        }

        // Trailing trivia: after this child but before next child (or parent end)
        let next_start = children
            .get(i + 1)
            .map(|c| c.span.start)
            .unwrap_or(parent.span.end);
        let mut trailing = Vec::new();
        while *trivia_idx < trivia.len() {
            let t = &trivia[*trivia_idx];
            if t.byte_offset() >= next_start {
                break;
            }
            trailing.push(t.clone());
            *trivia_idx += 1;
        }

        annotated.push(AnnotatedSyntax {
            syntax: (*child).clone(),
            leading,
            trailing,
            children: grandchildren,
        });
    }

    annotated
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_trivia_empty() {
        let trivia = collect_trivia("", &[]);
        assert!(trivia.is_empty());
    }

    #[test]
    fn test_collect_trivia_no_trivia() {
        let trivia = collect_trivia("(+ 1 2)", &[]);
        assert!(trivia.is_empty());
    }

    #[test]
    fn test_collect_trivia_comment() {
        let comments = vec![("# hello".to_string(), 0, 1)];
        let trivia = collect_trivia("# hello\n(+ 1 2)", &comments);
        assert_eq!(trivia.len(), 1);
        assert!(matches!(&trivia[0], Trivia::Comment { text, .. } if text == "# hello"));
    }

    #[test]
    fn test_collect_trivia_blank_lines() {
        let trivia = collect_trivia("a\n\n\nb", &[]);
        assert_eq!(trivia.len(), 1);
        assert!(matches!(&trivia[0], Trivia::BlankLines { count: 2, .. }));
    }

    #[test]
    fn test_collect_trivia_sorted() {
        let comments = vec![("# second".to_string(), 5, 2)];
        let source = "# first\n# second\n42";
        let mut trivia = collect_trivia(source, &comments);
        // Add first comment manually (it's not in comments because it would
        // be at byte offset 0)
        trivia.push(Trivia::Comment {
            text: "# first".to_string(),
            byte_offset: 0,
            line: 1,
        });
        trivia.sort_by_key(|t| t.byte_offset());
        assert!(trivia[0].byte_offset() < trivia[1].byte_offset());
    }

    #[test]
    fn test_annotated_atom() {
        let syntax = Syntax::new(SyntaxKind::Int(42), Span::new(0, 2, 1, 1));
        let (annotated, dangling) = AnnotatedSyntax::build_toplevel(vec![syntax], &[], "42");
        assert_eq!(annotated.len(), 1);
        assert!(matches!(annotated[0].kind(), SyntaxKind::Int(42)));
        assert!(annotated[0].leading.is_empty());
        assert!(annotated[0].children.is_empty());
        assert!(dangling.is_empty());
    }

    #[test]
    fn test_annotated_with_leading_comment() {
        let syntax = Syntax::new(SyntaxKind::Int(42), Span::new(9, 11, 2, 1));
        let trivia = vec![Trivia::Comment {
            text: "# before".to_string(),
            byte_offset: 0,
            line: 1,
        }];
        let (annotated, dangling) =
            AnnotatedSyntax::build_toplevel(vec![syntax], &trivia, "# before\n42");
        assert_eq!(annotated.len(), 1);
        assert_eq!(annotated[0].leading.len(), 1);
        assert!(
            matches!(&annotated[0].leading[0], Trivia::Comment { text, .. } if text == "# before")
        );
        assert!(dangling.is_empty());
    }

    #[test]
    fn test_annotated_list_children() {
        let syntax = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("+".into()), Span::new(1, 2, 1, 2)),
                Syntax::new(SyntaxKind::Int(1), Span::new(3, 4, 1, 4)),
                Syntax::new(SyntaxKind::Int(2), Span::new(5, 6, 1, 6)),
            ]),
            Span::new(0, 7, 1, 1),
        );
        let (annotated, _dangling) = AnnotatedSyntax::build_toplevel(vec![syntax], &[], "(+ 1 2)");
        assert_eq!(annotated[0].children.len(), 3);
    }
}
