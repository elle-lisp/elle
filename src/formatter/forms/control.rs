// audited: 2026-10-27
//! Control-flow forms: `cond`, `match`, `case`, `while`, `each`.
//!
//! docs/fmt.md

use super::{format_body, format_generic_call, is_trivial, parens};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{
    comment_between, format_annotated, format_pair_below, format_trailing_trivia,
    format_without_trailing,
};
use crate::formatter::trivia::AnnotatedSyntax;

// ── cond ───────────────────────────────────────────────────────

/// `(cond test1 body1 test2 body2 default)` — flat pairs.
///
/// Always break. Each test-body pair on its own line. Trivial body stays
/// with test when it fits; any other body breaks +2. Odd trailing element is
/// the default.
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

    parens(Doc::concat([head, Doc::HardBreak, clauses]))
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

    parens(Doc::concat([
        head,
        Doc::Space,
        expr,
        Doc::HardBreak,
        clauses,
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

    // Test always stays with head.
    let header = Doc::concat([head, Doc::Space, test]);
    if body_children.len() == 1 {
        // Single body: try inline, break before body if needed.
        parens(Doc::concat([header, Doc::Break, body]).group())
    } else {
        // Multiple body expressions: always break.
        parens(Doc::concat([header, Doc::HardBreak, body]))
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

    parens(Doc::concat([header, Doc::HardBreak, body]))
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

    parens(Doc::concat([
        Doc::concat([head, Doc::Break, expr]).group(),
        Doc::HardBreak,
        clauses,
    ]))
}

/// Format flat alternating test/body pairs.
///
/// A trivial body goes beside its test inside a Group of its own, so it moves
/// below the test, +2, when it does not fit there. Any other body, and a body
/// with a comment before it, breaks +2. An odd trailing element (default
/// clause) stands alone.
fn format_flat_pairs(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Vec<Doc> {
    children
        .chunks(2)
        .map(|pair| match pair {
            [test, body] if is_trivial(body) && !comment_between(test, body) => Doc::concat([
                format_annotated(test, source, config),
                Doc::concat([Doc::Break, format_without_trailing(body, source, config)])
                    .nest(1)
                    .group(),
                format_trailing_trivia(body),
            ]),
            [test, body] => format_pair_below(test, body, source, config),
            [default] => format_annotated(default, source, config),
            _ => unreachable!("chunks(2) yields one or two"),
        })
        .collect()
}
