// audited: 2026-10-27
//! The handler for each form the formatter knows, and the helpers they share.
//!
//! src/formatter/README.md
//!
//! Each handler receives the children of a list form, the head symbol as
//! `children[0]`, and returns a Doc for the whole form, parentheses included.
//! Children are positional: the handler knows which child is the name, the
//! parameters or the body. Each child goes through `format_annotated`, which
//! keeps its comments and blank lines.
//!
//! The handlers are grouped by shape:
//!
//! - `define` — `def`, `defn`, `fn`, `let`, `defmacro`.
//! - `body` — `if`, and the macros that take a header and then a body.
//! - `control` — `cond`, `match`, `case`, `while`, `each`.
//! - `misc` — `begin`, `forever`, `block`, `parameterize`, threading, `try`,
//!   `assign`.
//! - `call` — the generic call, which every other handler falls back to.

mod body;
mod call;
mod control;
mod define;
mod misc;

pub(super) use body::{format_body_macro, format_if};
pub(super) use call::format_generic_call;
pub(super) use control::{format_case, format_cond, format_each, format_match, format_while};
pub(super) use define::{format_def, format_defmacro, format_fn, format_let};
pub(super) use misc::{
    format_assign, format_begin, format_block, format_forever, format_parameterize,
    format_threading, format_try,
};

use super::config::FormatterConfig;
use super::doc::Doc;
use super::format::format_annotated;
use super::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

// ── Shared predicates ──────────────────────────────────────────

/// Check if a node is a string literal (for docstring detection).
pub(super) fn is_string_literal(node: &AnnotatedSyntax) -> bool {
    matches!(
        node.syntax.kind,
        SyntaxKind::String(_) | SyntaxKind::StringMut(_)
    )
}

/// Check if a node is a collection type (List, Array, etc.).
pub(super) fn is_collection(node: &AnnotatedSyntax) -> bool {
    matches!(
        node.syntax.kind,
        SyntaxKind::List(_)
            | SyntaxKind::Array(_)
            | SyntaxKind::ArrayMut(_)
            | SyntaxKind::Struct(_)
            | SyntaxKind::StructMut(_)
            | SyntaxKind::Set(_)
            | SyntaxKind::SetMut(_)
    )
}

/// A node is "trivial" if it is structurally shallow — at most 2 levels
/// of nested lists. A trivial body may share its test's line in a `cond`,
/// `match` or `case` pair, and its header's line in `if` and a body macro.
///
/// Depth budget: each compound node (list, collection) costs 1 level.
/// Atoms are free. Budget of 3 allows e.g. `(if (nil? x) a b)` (2 levels)
/// but rejects `(each x in xs (unless (nil? x) (push ...)))` (3+ levels).
pub(super) fn is_trivial(node: &AnnotatedSyntax) -> bool {
    is_trivial_depth(node, 3)
}

fn is_trivial_depth(node: &AnnotatedSyntax, budget: usize) -> bool {
    if budget == 0 {
        return false;
    }
    match &node.syntax.kind {
        // Atoms are always trivial (no depth cost)
        SyntaxKind::Nil
        | SyntaxKind::Bool(_)
        | SyntaxKind::Int(_)
        | SyntaxKind::Float(_)
        | SyntaxKind::Symbol(_)
        | SyntaxKind::Keyword(_)
        | SyntaxKind::String(_)
        | SyntaxKind::StringMut(_) => true,

        // A list costs 1 depth level
        SyntaxKind::List(_) => node
            .children
            .iter()
            .all(|c| is_trivial_depth(c, budget - 1)),

        // Collections cost 1 depth level
        SyntaxKind::Array(_)
        | SyntaxKind::ArrayMut(_)
        | SyntaxKind::Struct(_)
        | SyntaxKind::StructMut(_)
        | SyntaxKind::Set(_)
        | SyntaxKind::SetMut(_)
        | SyntaxKind::Bytes(_)
        | SyntaxKind::BytesMut(_) => node
            .children
            .iter()
            .all(|c| is_trivial_depth(c, budget - 1)),

        // Reader macros cost 1 depth level
        SyntaxKind::Quote(_)
        | SyntaxKind::Quasiquote(_)
        | SyntaxKind::Unquote(_)
        | SyntaxKind::UnquoteSplicing(_)
        | SyntaxKind::Splice(_) => node
            .children
            .first()
            .is_none_or(|c| is_trivial_depth(c, budget - 1)),

        SyntaxKind::SyntaxLiteral(_) => true,
    }
}

// ── Shared body/clause builders ────────────────────────────────

/// A form's parentheses around `inner`. The indent aligns at the open paren,
/// and `inner` nests +2 from it, so a form indents from its own paren.
pub(super) fn parens(inner: Doc) -> Doc {
    Doc::align(Doc::concat([Doc::text("("), inner.nest(1), Doc::text(")")]))
}

/// Format a sequence of body expressions separated by HardBreaks.
///
/// CommentBreak (emitted after trailing comments by format_annotated)
/// is absorbed by the inter-sibling HardBreak, so no special-casing needed.
pub(super) fn format_body(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.is_empty() {
        return Doc::empty();
    }
    let mut parts = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            parts.push(Doc::HardBreak);
        }
        parts.push(format_annotated(child, source, config));
    }
    Doc::concat(parts)
}
