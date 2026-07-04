//! Sequencing and remaining special forms: `begin`/`do`/`defer`, `forever`,
//! `block`, `parameterize`, threading (`->` and friends), `try`/`protect`,
//! `assign`.

use super::{format_body, format_generic_call};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_trailing_trivia};
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

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([head, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ]))
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
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::Break, body]).nest(1).group(),
            Doc::text(")"),
        ]))
    } else {
        let body = format_body(body_children, source, config);
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::HardBreak, body]).nest(1),
            Doc::text(")"),
        ]))
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

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([head, Doc::text(" "), name, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ]))
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

    // Emit the bindings list's own trailing inline comment (built from its
    // children above, which skips the node's own trailing trivia) — otherwise
    // a comment after the bindings `)` is dropped.
    let bindings_trivia = format_trailing_trivia(bindings_node);

    let body = format_body(&children[2..], source, config);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([
            head,
            Doc::text(" "),
            bindings,
            bindings_trivia,
            Doc::HardBreak,
            body,
        ])
        .nest(1),
        Doc::text(")"),
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
    let val = format_annotated(&children[1], source, config);
    let steps: Vec<Doc> = children[2..]
        .iter()
        .map(|c| format_annotated(c, source, config))
        .collect();

    // Align val and all steps to the column after "(-> "
    let mut all = Vec::with_capacity(steps.len() + 1);
    all.push(val);
    all.extend(steps);

    Doc::concat([
        Doc::text("("),
        head,
        Doc::text(" "),
        Doc::align(Doc::join_hardbreak(all)),
        Doc::text(")"),
    ])
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
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::intersperse([head, body]).nest(1).group(),
            Doc::text(")"),
        ]))
    } else {
        // Multiple sub-forms (e.g. body + catch/finally): break
        let body = format_body(body_children, source, config);
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::HardBreak, body]).nest(1),
            Doc::text(")"),
        ]))
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

    let elems: Vec<Doc> = children
        .iter()
        .map(|c| format_annotated(c, source, config))
        .collect();

    Doc::concat([
        Doc::text("("),
        Doc::intersperse(elems).nest(1).group(),
        Doc::text(")"),
    ])
}
