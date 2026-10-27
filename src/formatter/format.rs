// audited: 2026-10-25
//! Doc generator: walks AnnotatedSyntax trees, produces Doc trees.
//!
//! docs/fmt.md
//! src/formatter/README.md
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
//! form-specific rules from `forms/`. Unknown forms fall through to
//! generic call formatting.

use super::config::FormatterConfig;
use super::doc::Doc;
use super::trivia::{AnnotatedSyntax, Trivia};
use crate::reader::escape::StringLiteral;
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

    // Format all forms, separated by a HardBreak. Leading trivia supplies no
    // preceding break of its own (see `emit_leading`), so the separator is
    // emitted unconditionally between siblings.
    for (i, form) in forms.iter().enumerate() {
        if i > 0 {
            all_docs.push(Doc::hardbreak());
        }
        all_docs.push(format_annotated(form, source, config));
    }

    // Dangling trivia ends in a comment, or is empty: the attachment pass drops
    // the blank lines at the end of the file.
    if !dangling.is_empty() {
        if !all_docs.is_empty() {
            all_docs.push(Doc::hardbreak());
        }
        emit_leading(dangling, &mut all_docs);
    }

    if all_docs.is_empty() {
        Doc::empty()
    } else {
        Doc::concat(all_docs)
    }
}

// ── Trivia ─────────────────────────────────────────────────────

fn has_comment(trivia: &[Trivia]) -> bool {
    trivia.iter().any(|t| matches!(t, Trivia::Comment { .. }))
}

/// Emit leading trivia (comments + blank lines) into `parts`.
///
/// Each comment becomes `text + HardBreak`, and each blank line a BlankLine.
/// A FreshLine opens a list that holds a comment: where the caller's separator
/// has started a line it does nothing, and where the caller joined this node
/// to the text before it, it starts the line the comment needs.
fn emit_leading(leading: &[Trivia], parts: &mut Vec<Doc>) {
    if has_comment(leading) {
        parts.push(Doc::FreshLine);
    }
    for t in leading {
        match t {
            Trivia::Comment { text, .. } => {
                parts.push(Doc::text(text));
                parts.push(Doc::hardbreak());
            }
            Trivia::BlankLines { count, .. } => {
                for _ in 0..count.capped_at(2) {
                    parts.push(Doc::BlankLine);
                }
            }
        }
    }
}

/// Format only the leading trivia of a node, for a handler that lays out the
/// node's children itself.
pub(super) fn format_leading_trivia(node: &AnnotatedSyntax) -> Doc {
    let mut parts = Vec::new();
    emit_leading(&node.leading, &mut parts);
    Doc::concat(parts)
}

/// Format a node with its leading and trailing trivia.
///
/// Every AnnotatedSyntax passes through here to ensure trivia is
/// never lost. The trivia is emitted around the node's own Doc.
pub(super) fn format_annotated(
    node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    Doc::concat([
        format_without_trailing(node, source, config),
        format_trailing_trivia(node),
    ])
}

/// Format a node with leading trivia + syntax, but WITHOUT trailing trivia.
///
/// A handler that puts the node inside a Group or a Nest places the trailing
/// trivia itself, with `format_trailing_trivia`. Inside a Group, a comment
/// would break the Group; inside a Nest, the CommentBreak would indent the
/// element after the node.
pub(super) fn format_without_trailing(
    node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let mut parts = Vec::new();
    emit_leading(&node.leading, &mut parts);
    parts.push(format_syntax(node, source, config));
    Doc::concat(parts)
}

/// Format only the trailing and closing trivia of a node: the inline comment,
/// then the own-line comments and blank lines before the parent's close.
///
/// Comments extend to end-of-line, so a CommentBreak follows the last one.
/// It absorbs the separator after it, so a sibling's HardBreak adds no blank
/// line, and the closing delimiter of a parent goes on the line below.
pub(super) fn format_trailing_trivia(node: &AnnotatedSyntax) -> Doc {
    let mut parts = Vec::new();
    for t in &node.trailing {
        if let Trivia::Comment { text, .. } = t {
            parts.push(Doc::text("  "));
            parts.push(Doc::text(text));
        }
    }
    // Whether the current line holds text, so a comment or a blank line needs
    // a HardBreak before it.
    let mut line_holds_text = true;
    for t in &node.closing {
        match t {
            Trivia::Comment { text, .. } => {
                if line_holds_text {
                    parts.push(Doc::hardbreak());
                }
                parts.push(Doc::text(text));
                line_holds_text = true;
            }
            Trivia::BlankLines { count, .. } => {
                if line_holds_text {
                    parts.push(Doc::hardbreak());
                    line_holds_text = false;
                }
                for _ in 0..count.capped_at(2) {
                    parts.push(Doc::BlankLine);
                }
            }
        }
    }
    if has_comment(&node.trailing) || has_comment(&node.closing) {
        parts.push(Doc::comment_break());
    }
    Doc::concat(parts)
}

// ── Pairs ──────────────────────────────────────────────────────

/// A key and its value: a `cond` test and body, a binding, a struct entry, a
/// keyword argument. The value goes beside its key unless a comment comes
/// between them, and then it goes below, +2.
pub(super) fn format_pair(
    key: &AnnotatedSyntax,
    value: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if has_comment(&key.trailing) || has_comment(&value.leading) {
        format_pair_below(key, value, source, config)
    } else {
        Doc::concat([
            format_annotated(key, source, config),
            Doc::Space,
            format_annotated(value, source, config),
        ])
    }
}

/// A key with its value below it, +2. The key's trailing trivia sits inside
/// the Nest, so a CommentBreak there starts the value's line at +2. The
/// value's sits outside, so the element after the pair lines up with the key.
pub(super) fn format_pair_below(
    key: &AnnotatedSyntax,
    value: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    Doc::concat([
        format_without_trailing(key, source, config),
        Doc::concat([
            format_trailing_trivia(key),
            Doc::HardBreak,
            format_without_trailing(value, source, config),
        ])
        .nest(1),
        format_trailing_trivia(value),
    ])
}

// ── Core recursive walk ────────────────────────────────────────

/// Format a single Syntax node (no trivia).
///
/// Dispatches on SyntaxKind. For lists, checks the head symbol for
/// special-form dispatch.
fn format_syntax(node: &AnnotatedSyntax, source: &str, config: &FormatterConfig) -> Doc {
    match &node.syntax.kind {
        // ── Atoms ────────────────────────────────────────────
        SyntaxKind::Nil => Doc::text("nil"),
        SyntaxKind::Bool(b) => Doc::text(if *b { "true" } else { "false" }),
        SyntaxKind::Int(_) | SyntaxKind::Float(_) => {
            // Preserve original source representation (e.g. 1.7e308, 0xff)
            let span = &node.syntax.span;
            let (start, end) = (span.start as usize, span.end as usize);
            if start < source.len() && end <= source.len() {
                Doc::text(&source[start..end])
            } else {
                match &node.syntax.kind {
                    SyntaxKind::Int(n) => Doc::text(n.to_string()),
                    SyntaxKind::Float(f) => Doc::text(format!("{:e}", f)),
                    _ => unreachable!(),
                }
            }
        }
        SyntaxKind::Symbol(s) => Doc::text(s.to_string()),
        SyntaxKind::Keyword(s) => Doc::text(format!(":{}", s)),
        SyntaxKind::String(_) | SyntaxKind::StringMut(_) => {
            // Slice from source to preserve the raw literal (escapes, quotes).
            // SyntaxKind::String stores the unescaped value, not the source text.
            let span = &node.syntax.span;
            let (start, end) = (span.start as usize, span.end as usize);
            if start < source.len() && end <= source.len() {
                Doc::text(&source[start..end])
            } else {
                // Synthetic or detached span — print the string itself.
                match &node.syntax.kind {
                    SyntaxKind::String(s) => Doc::text(StringLiteral(s).to_string()),
                    SyntaxKind::StringMut(s) => Doc::text(format!("@{}", StringLiteral(s))),
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
            "begin" | "do" | "defer" => {
                return super::forms::format_begin(children, source, config)
            }
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
        Doc::align(Doc::fill(elems)),
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

    let pair_docs: Vec<Doc> = node
        .children
        .chunks(2)
        .map(|pair| match pair {
            [key, value] => format_pair(key, value, source, config),
            [key] => format_annotated(key, source, config),
            _ => unreachable!("chunks(2) yields one or two"),
        })
        .collect();

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

#[cfg(test)]
mod tests;
