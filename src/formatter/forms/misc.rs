// audited: 2026-10-27
//! Layout for `begin`/`do`/`defer`, `forever`, `block`, `parameterize`, the
//! threading macros, `try`/`protect` and `assign`.
//!
//! docs/fmt.md

use super::{format_body, format_generic_call, parens};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_leading_trivia, format_trailing_trivia};
use crate::formatter::trivia::AnnotatedSyntax;

// ── begin ──────────────────────────────────────────────────────

/// `(begin body...)` — always break. Each expression on its own line, +2 indent.
pub(in crate::formatter) fn format_begin(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 2 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let body = format_body(&children[1..], source, config);

    parens(Doc::concat([head, Doc::HardBreak, body]))
}

// ── forever ────────────────────────────────────────────────────

/// `(forever body...)` — infinite loop. Single body: try inline. Multi: break like begin.
pub(in crate::formatter) fn format_forever(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 2 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let body_children = &children[1..];

    if body_children.len() == 1 {
        let body = format_annotated(&body_children[0], source, config);
        parens(Doc::concat([head, Doc::Break, body]).group())
    } else {
        let body = format_body(body_children, source, config);
        parens(Doc::concat([head, Doc::HardBreak, body]))
    }
}

// ── block ──────────────────────────────────────────────────────

/// `(block :name body...)` — like begin, with :name on same line as block.
pub(in crate::formatter) fn format_block(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let name = format_annotated(&children[1], source, config);
    let body = format_body(&children[2..], source, config);

    parens(Doc::concat([head, Doc::Space, name, Doc::HardBreak, body]))
}

// ── parameterize ──────────────────────────────────────────────

/// `(parameterize ((var val) ...) body...)` — bindings each on a new line,
/// aligned to the first binding via Align.
pub(in crate::formatter) fn format_parameterize(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);

    // children[1] is the bindings list ((var val) ...)
    let bindings_node = &children[1];
    let binding_docs: Vec<Doc> = bindings_node
        .children
        .iter()
        .map(|c| format_annotated(c, source, config))
        .collect();

    let bindings = if binding_docs.is_empty() {
        Doc::text("()")
    } else {
        // Align binding entries to the column after "(parameterize ("
        Doc::concat([
            Doc::text("("),
            Doc::align(Doc::join_hardbreak(binding_docs)),
            Doc::text(")"),
        ])
    };

    // The bindings are built from the list's children above, so the trivia of
    // the list itself, above it and after its `)`, is emitted here.
    let bindings = Doc::concat([format_leading_trivia(bindings_node), bindings]);
    let bindings_trivia = format_trailing_trivia(bindings_node);

    let body = format_body(&children[2..], source, config);

    parens(Doc::concat([
        head,
        Doc::Space,
        bindings,
        bindings_trivia,
        Doc::HardBreak,
        body,
    ]))
}

// ── Threading macros ─────────────────────────────────────────

/// `(-> val step...)` — always break. Steps align with value.
pub(in crate::formatter) fn format_threading(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let steps = children[1..]
        .iter()
        .map(|c| format_annotated(c, source, config));

    // Align val and all steps to the column after "(-> "
    parens(Doc::concat([
        head,
        Doc::Space,
        Doc::align(Doc::join_hardbreak(steps)),
    ]))
}

// ── try / protect ──────────────────────────────────────────────

/// `(try body (catch pattern handler))` or `(protect body (finally cleanup))`.
///
/// Single short body: try inline. Multiple or long body: break.
pub(in crate::formatter) fn format_try(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let body_children = &children[1..];

    if body_children.len() == 1 {
        // Single body: try inline
        let body = format_annotated(&body_children[0], source, config);
        parens(Doc::intersperse([head, body]).group())
    } else {
        // Multiple sub-forms (e.g. body + catch/finally): break
        let body = format_body(body_children, source, config);
        parens(Doc::concat([head, Doc::HardBreak, body]))
    }
}

// ── assign ─────────────────────────────────────────────────────

/// `(assign name value)` — inline if fits.
pub(in crate::formatter) fn format_assign(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let elems = children.iter().map(|c| format_annotated(c, source, config));

    parens(Doc::intersperse(elems).group())
}
