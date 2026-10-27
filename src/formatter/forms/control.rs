// audited: 2026-10-25
//! Control-flow forms: `if`, `cond`, `match`, `while`, `when`/`unless`,
//! `each`, `case`.
//!
//! docs/fmt.md

use super::{format_body, format_generic_call, is_trivial};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_pair, format_pair_below};
use crate::formatter::trivia::AnnotatedSyntax;

// ── if ─────────────────────────────────────────────────────────

/// `(if test then else?)`.
///
/// Inline if fits. When breaking:
/// - Trivial branches: columnar (align to first arg).
/// - Compound branches: +2 body indent.
pub(in crate::formatter) fn format_if(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let test = format_annotated(&children[1], source, config);
    let then = format_annotated(&children[2], source, config);

    let branches = &children[2..];
    let trivial = branches.iter().all(is_trivial);

    if children.len() <= 3 {
        // (if test then) — same as when
        if trivial {
            let header = Doc::concat([head, Doc::Space, test]);
            Doc::align(Doc::concat([
                Doc::text("("),
                Doc::concat([header, Doc::Break, then]).nest(1).group(),
                Doc::text(")"),
            ]))
        } else {
            Doc::align(Doc::concat([
                Doc::text("("),
                Doc::concat([head, Doc::Space, test, Doc::HardBreak, then]).nest(1),
                Doc::text(")"),
            ]))
        }
    } else {
        let else_ = format_annotated(&children[3], source, config);

        if trivial {
            // Trivial branches: test stays with head, branches break to +2
            let header = Doc::concat([head, Doc::Space, test]);
            Doc::align(Doc::concat([
                Doc::text("("),
                Doc::concat([header, Doc::Break, then, Doc::Break, else_])
                    .nest(1)
                    .group(),
                Doc::text(")"),
            ]))
        } else {
            // Compound branches: always break, +2 indent relative to (if.
            // head+test inside Nest so CommentBreak absorption uses correct indent.
            Doc::align(Doc::concat([
                Doc::text("("),
                Doc::concat([
                    head,
                    Doc::Space,
                    test,
                    Doc::HardBreak,
                    then,
                    Doc::HardBreak,
                    else_,
                ])
                .nest(1),
                Doc::text(")"),
            ]))
        }
    }
}

// ── cond ───────────────────────────────────────────────────────

/// `(cond test1 body1 test2 body2 default)` — flat pairs.
///
/// Always break. Each test-body pair on its own line. Trivial body stays
/// with test; compound body breaks +2. Odd trailing element is the default.
pub(in crate::formatter) fn format_cond(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 2 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let pairs = format_flat_pairs(&children[1..], source, config);
    let clauses = Doc::join_hardbreak(pairs);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([head, Doc::HardBreak, clauses]).nest(1),
        Doc::text(")"),
    ]))
}

// ── match ──────────────────────────────────────────────────────

/// `(match expr pat1 body1 pat2 body2 default)` — flat pairs after expr.
pub(in crate::formatter) fn format_match(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let expr = format_annotated(&children[1], source, config);
    let pairs = format_flat_pairs(&children[2..], source, config);
    let clauses = Doc::join_hardbreak(pairs);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([head, Doc::Space, expr, Doc::HardBreak, clauses]).nest(1),
        Doc::text(")"),
    ]))
}

// ── while ──────────────────────────────────────────────────────

/// `(while test body...)` — break if body has >1 expression.
pub(in crate::formatter) fn format_while(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let test = format_annotated(&children[1], source, config);
    let body_children = &children[2..];

    let body = format_body(body_children, source, config);

    if body_children.len() == 1 {
        // Single body: try inline, break before body if needed.
        // Test always stays with head.
        let header = Doc::concat([head, Doc::Space, test]);
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([header, Doc::Break, body]).nest(1).group(),
            Doc::text(")"),
        ]))
    } else {
        // Multiple body expressions: always break.
        // Test always stays with head.
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::Space, test, Doc::HardBreak, body]).nest(1),
            Doc::text(")"),
        ]))
    }
}

// ── when / unless ──────────────────────────────────────────────

/// `(when test body...)` or `(unless test body...)`.
///
/// Trivial body (single, no nested body forms): columnar alignment.
/// Compound body: +2 indent.
pub(in crate::formatter) fn format_when(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let test = format_annotated(&children[1], source, config);
    let body_children = &children[2..];
    let body = format_body(body_children, source, config);

    let trivial = body_children.len() == 1 && is_trivial(&body_children[0]);

    if trivial {
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::Space, test, Doc::Break, body])
                .nest(1)
                .group(),
            Doc::text(")"),
        ]))
    } else {
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::Space, test, Doc::HardBreak, body]).nest(1),
            Doc::text(")"),
        ]))
    }
}

// ── each ───────────────────────────────────────────────────────

/// `(each item in collection body...)`.
///
/// ```lisp
/// (each item in collection
///   body)
/// ```
pub(in crate::formatter) fn format_each(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    // Two forms: (each item in collection body...) or (each item collection body...)
    let has_in = children.get(2).and_then(|c| c.syntax.as_symbol()) == Some("in");

    let (coll_idx, body_start) = if has_in { (3, 4) } else { (2, 3) };

    if children.len() <= body_start {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let item = format_annotated(&children[1], source, config);
    let coll = format_annotated(&children[coll_idx], source, config);
    let body = format_body(&children[body_start..], source, config);

    // Header: each item [in] collection — always on one line
    let header = if has_in {
        let in_kw = format_annotated(&children[2], source, config);
        Doc::concat([head, Doc::Space, item, Doc::Space, in_kw, Doc::Space, coll])
    } else {
        Doc::concat([head, Doc::Space, item, Doc::Space, coll])
    };

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([header, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ]))
}

// ── case ───────────────────────────────────────────────────────

/// `(case expr key result ...)` — always break. Flat alternating pairs.
pub(in crate::formatter) fn format_case(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let expr = format_annotated(&children[1], source, config);
    let pairs = format_flat_pairs(&children[2..], source, config);
    let clauses = Doc::join_hardbreak(pairs);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([
            Doc::concat([head, Doc::Break, expr]).group(),
            Doc::HardBreak,
            clauses,
        ])
        .nest(1),
        Doc::text(")"),
    ]))
}

/// Format flat alternating test/body pairs.
///
/// Trivial body stays on the same line as test. Compound body breaks +2.
/// An odd trailing element (default clause) stands alone.
fn format_flat_pairs(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Vec<Doc> {
    children
        .chunks(2)
        .map(|pair| match pair {
            [test, body] if is_trivial(body) => format_pair(test, body, source, config),
            [test, body] => format_pair_below(test, body, source, config),
            [default] => format_annotated(default, source, config),
            _ => unreachable!("chunks(2) yields one or two"),
        })
        .collect()
}
