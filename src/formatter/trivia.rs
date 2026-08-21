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

use super::pos::{BlankCount, ByteOffset, LineNum};
use crate::syntax::{Span, Syntax, SyntaxKind};

// ── Trivia types ──────────────────────────────────────────────

/// A comment's text and source position, handed from the lexer/comment layer
/// to [`collect_trivia`]. A named replacement for what was an anonymous
/// `(String, usize, u32)` tuple, so the offset and line can't be transposed.
#[derive(Debug, Clone)]
pub struct CommentInfo {
    pub text: String,
    pub offset: ByteOffset,
    pub line: LineNum,
}

/// A piece of source trivia — a comment or blank lines.
/// Positioned by byte offset for attachment to Syntax nodes.
#[derive(Debug, Clone)]
pub enum Trivia {
    /// A line comment: `# text` or `## doc text`.
    /// `text` includes the `#` prefix, with trailing newline stripped.
    Comment {
        text: String,
        byte_offset: ByteOffset,
        line: LineNum,
    },
    /// One or more consecutive blank lines.
    BlankLines {
        count: BlankCount,
        byte_offset: ByteOffset,
        /// Line number of the first blank line.
        line: LineNum,
    },
}

impl Trivia {
    /// Byte offset where this trivia starts in the source.
    pub fn byte_offset(&self) -> ByteOffset {
        match self {
            Trivia::Comment { byte_offset, .. } => *byte_offset,
            Trivia::BlankLines { byte_offset, .. } => *byte_offset,
        }
    }

    /// Line number of this trivia.
    pub fn line(&self) -> LineNum {
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
pub fn collect_trivia(source: &str, comments: &[CommentInfo]) -> Vec<Trivia> {
    let mut trivia: Vec<Trivia> = Vec::new();

    // Add comments from the lexer
    for ci in comments {
        trivia.push(Trivia::Comment {
            text: ci.text.clone(),
            byte_offset: ci.offset,
            line: ci.line,
        });
    }

    // Scan for blank lines. The running offset/line/count stay raw integers
    // and are wrapped in their position newtypes only at the push site.
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
                        count: BlankCount::new(blank_count),
                        byte_offset: ByteOffset::new(boff),
                        line: LineNum::new(line),
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
            count: BlankCount::new(blank_count),
            byte_offset: ByteOffset::new(boff),
            line: LineNum::new(line),
        });
    }

    // Sort by byte offset
    trivia.sort_by_key(|t| t.byte_offset());
    trivia
}

// ── Attachment pass ───────────────────────────────────────────

/// Line number (1-based) of a byte offset in `source`. A free function (not a
/// closure) so both `attach_trivia_to_forms` and `attach_to_children` share it.
/// Iterates `char_indices`, so `i` is a BYTE index — comparing a char index
/// (`chars().enumerate()`) against a byte offset would misplace every comment
/// after the first non-ASCII source byte.
fn line_at_offset(source: &str, offset: ByteOffset) -> LineNum {
    let mut line: u32 = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset.get() {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    LineNum::new(line)
}

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

    // Pre-compute span starts for looking ahead to next form
    let form_spans: Vec<(ByteOffset, ByteOffset)> = forms
        .iter()
        .map(|f| (ByteOffset::new(f.span.start), ByteOffset::new(f.span.end)))
        .collect();

    let mut all_attached = Vec::new();
    let mut trivia_idx: usize = 0;

    for (form_idx, form) in forms.into_iter().enumerate() {
        let span = form.span.clone();

        // Collect leading trivia: items before this form's span start
        let mut leading = Vec::new();
        while trivia_idx < trivia.len() {
            let t = &trivia[trivia_idx];
            if t.byte_offset() >= ByteOffset::new(span.start) {
                break;
            }
            leading.push(t.clone());
            trivia_idx += 1;
        }

        // Build children (recursively attach trivia within this form)
        let children = attach_to_children(&form, trivia, &mut trivia_idx, source);

        // Collect trailing trivia: comments on the same line as this form's end.
        // Blank lines and comments on later lines are left for the next form.
        // For the last form, don't attach trailing trivia (leave it dangling).
        let mut trailing = Vec::new();
        let is_last_form = form_idx + 1 >= form_spans.len();
        if !is_last_form {
            let form_end_line = line_at_offset(source, ByteOffset::new(span.end));
            let next_start = form_spans
                .get(form_idx + 1)
                .map(|(s, _)| *s)
                .unwrap_or(ByteOffset::MAX);

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
    source: &str,
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
            if t.byte_offset() >= ByteOffset::new(span.start) {
                break;
            }
            leading.push(t.clone());
            *trivia_idx += 1;
        }

        // Recurse into grandchildren
        let grandchildren = attach_to_children(child, trivia, trivia_idx, source);

        // Skip trivia items that fall inside this child's span but were
        // not consumed by grandchildren (e.g. blank lines inside strings).
        while *trivia_idx < trivia.len()
            && trivia[*trivia_idx].byte_offset() < ByteOffset::new(span.end)
        {
            *trivia_idx += 1;
        }

        // Trailing trivia: after this child but before the next child (or the
        // parent's close). Only a comment on the SAME line as the child ends is
        // inline-trailing; an own-line comment is a *leading* comment of the
        // following child, so it is left for the next child's leading collection
        // (matching the top-level pass). The last child has no following sibling,
        // so everything up to the parent's close attaches to it — otherwise the
        // caller's inside-span skip would drop it.
        let is_last = i + 1 == children.len();
        let next_start = children
            .get(i + 1)
            .map(|c| ByteOffset::new(c.span.start))
            .unwrap_or(ByteOffset::new(parent.span.end));
        let child_end_line = line_at_offset(source, ByteOffset::new(span.end));
        let mut trailing = Vec::new();
        while *trivia_idx < trivia.len() {
            let t = &trivia[*trivia_idx];
            if t.byte_offset() >= next_start {
                break;
            }
            if is_last {
                trailing.push(t.clone());
                *trivia_idx += 1;
            } else {
                match t {
                    Trivia::Comment { line, .. } if *line == child_end_line => {
                        trailing.push(t.clone());
                        *trivia_idx += 1;
                    }
                    _ => break,
                }
            }
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
mod tests;
