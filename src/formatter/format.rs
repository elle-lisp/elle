//! Doc generator: walks AnnotatedSyntax trees, produces Doc trees.
//!
//! This is the formatter's brain. It takes the annotated syntax tree
//! (where trivia is pre-attached to every node) and produces a Wadler
//! Doc tree. The Doc tree is then rendered to a string by `render.rs`.
//!
//! ## Design
//!
//! The walk is a pure function `AnnotatedSyntax → Doc` with no mutable
//! state. Trivia (comments, blank lines) is emitted from the annotated
//! tree — no separate CommentMap is consulted during the walk.
//!
//! For list forms, the generator dispatches on the head symbol to apply
//! form-specific rules from `forms.rs`. Unknown forms fall through to
//! generic call formatting.

use super::config::FormatterConfig;
use super::doc::Doc;
use super::trivia::{AnnotatedSyntax, Trivia};
use crate::syntax::SyntaxKind;

// ── Public entry point ─────────────────────────────────────────

/// Format a list of top-level forms into a single Doc.
///
/// Forms are separated by HardBreaks. Leading and trailing trivia
/// (comments, blank lines) are emitted around each form.
/// Dangling trivia (comments after the last form) is emitted at the end.
pub fn format_forms(
    forms: &[AnnotatedSyntax],
    dangling: &[Trivia],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let mut all_docs: Vec<Doc> = Vec::new();

    // Format all forms. Between forms, add a HardBreak only if the
    // next form has no leading trivia (which provides its own spacing).
    for (i, form) in forms.iter().enumerate() {
        if i > 0 && form.leading.is_empty() {
            all_docs.push(Doc::hardbreak());
        }
        all_docs.push(format_annotated(form, source, config));
    }

    // Emit dangling trivia (comments after the last form) without extra spacing
    let mut has_dangling_comments = false;
    for t in dangling {
        if let Trivia::Comment { .. } = t {
            has_dangling_comments = true;
            break;
        }
    }

    if has_dangling_comments {
        // If we have forms, add a hardbreak before the first dangling trivia
        if !all_docs.is_empty() {
            all_docs.push(Doc::hardbreak());
        }

        for t in dangling {
            if let Trivia::Comment { text, .. } = t {
                all_docs.push(Doc::text(text));
                all_docs.push(Doc::hardbreak());
            }
            // Note: BlankLines trivia in dangling is ignored.
            // The formatter ensures comments are consecutive without blank lines.
        }
    }

    if all_docs.is_empty() {
        Doc::empty()
    } else {
        Doc::concat(all_docs)
    }
}

// ── Core recursive walk ────────────────────────────────────────

/// Format a node with its leading and trailing trivia.
///
/// Every AnnotatedSyntax passes through here to ensure trivia is
/// never lost. The trivia is emitted around the node's own Doc.
pub(super) fn format_annotated(
    node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let mut parts = Vec::new();

    // Leading trivia: comments and blank lines before this node.
    //
    // The inter-form HardBreak from format_forms provides the newline
    // that ends the previous form's line. Leading trivia adds to that:
    // - BlankLines(n): emit n HardBreaks (n blank lines in output)
    // - Comment: emit HardBreak + text (comment on its own line)
    //
    // After all trivia, one final HardBreak separates the last trivia
    // from the form itself.
    if !node.leading.is_empty() {
        for t in &node.leading {
            match t {
                Trivia::Comment { text, .. } => {
                    parts.push(Doc::hardbreak());
                    parts.push(Doc::text(text));
                }
                Trivia::BlankLines { count, .. } => {
                    for _ in 0..(*count).min(2) {
                        parts.push(Doc::hardbreak());
                    }
                }
            }
        }
        // Separate from the form
        parts.push(Doc::hardbreak());
    }

    // The node itself
    parts.push(format_syntax(node, source, config));

    // Trailing trivia: inline comments and blank lines after this node.
    //
    // Comments extend to end-of-line, so they MUST be followed by a
    // newline — otherwise the next token gets eaten by the comment.
    //
    // BlankLines in trailing trivia are only emitted when followed by
    // a comment — they provide block-vs-inline context. Otherwise,
    // inter-sibling spacing is handled by the form formatters' own
    // HardBreaks, and trailing blank lines before close parens must
    // be stripped to maintain idempotency.
    let has_trailing_comments = node
        .trailing
        .iter()
        .any(|t| matches!(t, Trivia::Comment { .. }));
    let mut seen_break = false;
    let mut inline_done = false;
    for t in &node.trailing {
        match t {
            Trivia::Comment { text, .. } => {
                if !seen_break && !inline_done {
                    // Inline: same line as the form
                    parts.push(Doc::text("  "));
                    parts.push(Doc::text(text));
                    inline_done = true;
                } else {
                    // Block: own line
                    parts.push(Doc::hardbreak());
                    parts.push(Doc::text(text));
                }
            }
            Trivia::BlankLines { count, .. } => {
                seen_break = true;
                if has_trailing_comments {
                    for _ in 0..(*count).min(2) {
                        parts.push(Doc::hardbreak());
                    }
                }
            }
        }
    }

    // Comments extend to end-of-line, so they MUST be followed by a
    // newline. CommentBreak renders like HardBreak but is absorbed by
    // an adjacent HardBreak, preventing double-newline between siblings.
    if node
        .trailing
        .iter()
        .any(|t| matches!(t, Trivia::Comment { .. }))
    {
        parts.push(Doc::comment_break());
    }

    Doc::concat(parts)
}

/// Format a node with leading trivia + syntax, but WITHOUT trailing trivia.
///
/// Used for header elements (like params) whose trailing trivia (comments,
/// blank lines before the body) would poison `measure_flat` inside a Group,
/// forcing the header to break unnecessarily.
pub(super) fn format_without_trailing(
    node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let mut parts = Vec::new();

    // Leading trivia (same as format_annotated)
    if !node.leading.is_empty() {
        for t in &node.leading {
            match t {
                Trivia::Comment { text, .. } => {
                    parts.push(Doc::hardbreak());
                    parts.push(Doc::text(text));
                }
                Trivia::BlankLines { count, .. } => {
                    for _ in 0..(*count).min(2) {
                        parts.push(Doc::hardbreak());
                    }
                }
            }
        }
        parts.push(Doc::hardbreak());
    }

    // The node itself — no trailing trivia
    parts.push(format_syntax(node, source, config));
    Doc::concat(parts)
}

/// Format only the trailing trivia of a node.
///
/// Companion to `format_without_trailing`. Emit this after the header
/// group so trivia appears between header and body without affecting
/// the header's flat-width measurement.
pub(super) fn format_trailing_trivia(node: &AnnotatedSyntax) -> Doc {
    if node.trailing.is_empty() {
        return Doc::empty();
    }

    let mut parts = Vec::new();
    let has_trailing_comments = node
        .trailing
        .iter()
        .any(|t| matches!(t, Trivia::Comment { .. }));
    let mut seen_break = false;
    let mut inline_done = false;
    for t in &node.trailing {
        match t {
            Trivia::Comment { text, .. } => {
                if !seen_break && !inline_done {
                    parts.push(Doc::text("  "));
                    parts.push(Doc::text(text));
                    inline_done = true;
                } else {
                    parts.push(Doc::hardbreak());
                    parts.push(Doc::text(text));
                }
            }
            Trivia::BlankLines { count, .. } => {
                seen_break = true;
                if has_trailing_comments {
                    for _ in 0..(*count).min(2) {
                        parts.push(Doc::hardbreak());
                    }
                }
            }
        }
    }

    if has_trailing_comments {
        parts.push(Doc::comment_break());
    }

    Doc::concat(parts)
}

/// Format a single Syntax node (no trivia).
///
/// Dispatches on SyntaxKind. For lists, checks the head symbol for
/// special-form dispatch.
fn format_syntax(node: &AnnotatedSyntax, source: &str, config: &FormatterConfig) -> Doc {
    match &node.syntax.kind {
        // ── Atoms ────────────────────────────────────────────
        SyntaxKind::Nil => Doc::text("nil"),
        SyntaxKind::Bool(b) => Doc::text(if *b { "true" } else { "false" }),
        SyntaxKind::Int(n) => Doc::text(n.to_string()),
        SyntaxKind::Float(f) => Doc::text(f.to_string()),
        SyntaxKind::Symbol(s) => Doc::text(s.clone()),
        SyntaxKind::Keyword(s) => Doc::text(format!(":{}", s)),
        SyntaxKind::String(_) => {
            // Slice from source to preserve the raw literal (escapes, quotes).
            // SyntaxKind::String stores the unescaped value, not the source text.
            let span = &node.syntax.span;
            if span.start < source.len() && span.end <= source.len() {
                Doc::text(&source[span.start..span.end])
            } else {
                // Synthetic or detached span — fall back to escaped display
                match &node.syntax.kind {
                    SyntaxKind::String(s) => Doc::text(format!("\"{}\"", s.escape_default())),
                    _ => Doc::text("#<bad-string>"),
                }
            }
        }

        // ── Compound collections ─────────────────────────────
        SyntaxKind::List(_) => format_list(node, source, config),
        SyntaxKind::Array(_) => format_collection(node, "[", "]", source, config),
        SyntaxKind::ArrayMut(_) => format_collection(node, "@[", "]", source, config),
        SyntaxKind::Struct(_) => format_pairs(node, "{", "}", source, config),
        SyntaxKind::StructMut(_) => format_pairs(node, "@{", "}", source, config),
        SyntaxKind::Set(_) => format_collection(node, "|", "|", source, config),
        SyntaxKind::SetMut(_) => format_collection(node, "@|", "|", source, config),
        SyntaxKind::Bytes(_) => format_collection(node, "b[", "]", source, config),
        SyntaxKind::BytesMut(_) => format_collection(node, "@b[", "]", source, config),

        // ── Reader macros ────────────────────────────────────
        SyntaxKind::Quote(_) => format_reader_macro("'", node, source, config),
        SyntaxKind::Quasiquote(_) => format_reader_macro("`", node, source, config),
        SyntaxKind::Unquote(_) => format_reader_macro(",", node, source, config),
        SyntaxKind::UnquoteSplicing(_) => format_reader_macro(",;", node, source, config),
        SyntaxKind::Splice(_) => format_reader_macro(";", node, source, config),

        // ── Internal (should never appear in formatter input) ──
        SyntaxKind::SyntaxLiteral(_) => Doc::text("#<syntax-literal>"),
    }
}

// ── List dispatch ──────────────────────────────────────────────

/// Format a list form, dispatching on the head symbol.
fn format_list(node: &AnnotatedSyntax, source: &str, config: &FormatterConfig) -> Doc {
    let children = &node.children;

    if children.is_empty() {
        return Doc::text("()");
    }

    // Dispatch on the head symbol
    if let Some(sym) = children[0].syntax.as_symbol() {
        match sym {
            "def" | "defn" => return super::forms::format_def(children, source, config),
            "fn" => return super::forms::format_fn(children, source, config),
            "let" | "let*" | "letrec" => return super::forms::format_let(children, source, config),
            "if" => return super::forms::format_if(children, source, config),
            "cond" => return super::forms::format_cond(children, source, config),
            "match" => return super::forms::format_match(children, source, config),
            "while" => return super::forms::format_while(children, source, config),
            "defmacro" => return super::forms::format_defmacro(children, source, config),
            "begin" => return super::forms::format_begin(children, source, config),
            "forever" => return super::forms::format_forever(children, source, config),
            "block" => return super::forms::format_block(children, source, config),
            "parameterize" => return super::forms::format_parameterize(children, source, config),
            "->" | "->>" | "some->" | "some->>" => {
                return super::forms::format_threading(children, source, config)
            }
            "when" | "unless" => return super::forms::format_when(children, source, config),
            "and" | "or" | "not" | "emit" => {
                return super::forms::format_generic_call(children, source, config)
            }
            "each" => return super::forms::format_each(children, source, config),
            "case" => return super::forms::format_case(children, source, config),
            "try" | "protect" => return super::forms::format_try(children, source, config),
            "assign" => return super::forms::format_assign(children, source, config),
            _ => {}
        }
    }

    // Default: generic call
    super::forms::format_generic_call(children, source, config)
}

// ── Generic collection ─────────────────────────────────────────

/// Format a generic collection with delimiters.
///
/// Elements are separated by Break (space if flat, newline if broken).
/// The whole body is grouped — if it fits on one line, it stays flat.
fn format_collection(
    node: &AnnotatedSyntax,
    open: &str,
    close: &str,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if node.children.is_empty() {
        return Doc::text(format!("{}{}", open, close));
    }

    let elems: Vec<Doc> = node
        .children
        .iter()
        .map(|c| format_annotated(c, source, config))
        .collect();

    Doc::concat([
        Doc::text(open),
        Doc::align(Doc::intersperse(elems).group()),
        Doc::text(close),
    ])
}

// ── Pair-wise collection (structs) ─────────────────────────────

/// Format a key-value collection (structs). Elements are grouped in pairs.
/// Flat: `{:a 1 :b 2}`. Broken: one pair per line, +2 indent.
fn format_pairs(
    node: &AnnotatedSyntax,
    open: &str,
    close: &str,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if node.children.is_empty() {
        return Doc::text(format!("{}{}", open, close));
    }

    let children = &node.children;

    // Build pair docs: each pair is "key value" joined by a space
    let mut pair_docs: Vec<Doc> = Vec::new();
    let mut i = 0;
    while i < children.len() {
        let key = format_annotated(&children[i], source, config);
        i += 1;
        if i < children.len() {
            let val = format_annotated(&children[i], source, config);
            i += 1;
            pair_docs.push(Doc::concat([key, Doc::text(" "), val]));
        } else {
            pair_docs.push(key);
        }
    }

    Doc::concat([
        Doc::text(open),
        Doc::align(Doc::intersperse(pair_docs).group()),
        Doc::text(close),
    ])
}

// ── Reader macros ──────────────────────────────────────────────

/// Format a reader macro: `'x`, `` `x ``, `,x`, `;x`.
///
/// The prefix is prepended to the formatted inner form.
/// If the inner form breaks, the prefix stays on the same line
/// as the first element.
fn format_reader_macro(
    prefix: &str,
    node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if node.children.is_empty() {
        return Doc::text(prefix);
    }

    let inner = format_annotated(&node.children[0], source, config);
    Doc::concat([Doc::text(prefix), inner])
}
