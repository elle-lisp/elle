//! Per-special-form formatting rules.
//!
//! Each public function receives the children of a list form (including
//! the head symbol as `children[0]`) and returns a Doc for the entire
//! form including parentheses.
//!
//! ## Convention
//!
//! Children are positional — the handler knows which child is the name,
//! params, body, etc. Each child is formatted via `format_annotated`
//! which preserves its attached trivia (comments, blank lines).
//!
//! ## Layout
//!
//! The rules are grouped by shape rather than alphabetically:
//!
//! - `define` — binding/definitional forms (`def`, `defn`, `fn`, `let`,
//!   `defmacro`).
//! - `control` — control flow (`if`, `cond`, `match`, `when`, `case`,
//!   `while`, `each`).
//! - `misc` — sequencing and the remaining special forms (`begin`,
//!   `forever`, `block`, `parameterize`, threading, `try`, `assign`).
//! - `call` — the generic function-call fallback every other rule defers to.
//!
//! The small predicates and the body/clause helpers shared across those
//! groups live here in the parent module; submodules reach them via `super::`.

mod call;
mod control;
mod define;
mod misc;

pub(super) use call::format_generic_call;
pub(super) use control::{
    format_case, format_cond, format_each, format_if, format_match, format_when, format_while,
};
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
/// of nested lists. Trivial nodes stay on the same line in cond/match
/// pairs and get columnar alignment in if/when; deeply nested nodes break.
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
