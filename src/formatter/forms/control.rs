//! Control-flow forms: `if`, `cond`, `match`, `while`, `when`/`unless`,
//! `each`, `case`.

use super::{format_body, format_generic_call, is_trivial};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_trailing_trivia, format_without_trailing};
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
            let header = Doc::concat([head, Doc::text(" "), test]);
            Doc::align(Doc::concat([
                Doc::text("("),
                Doc::concat([header, Doc::Break, then]).nest(1).group(),
                Doc::text(")"),
            ]))
        } else {
            Doc::align(Doc::concat([
                Doc::text("("),
                head,
                Doc::text(" "),
                test,
                Doc::concat([Doc::HardBreak, then]).nest(1),
                Doc::text(")"),
            ]))
        }
    } else {
        let else_ = format_annotated(&children[3], source, config);

        if trivial {
            // Trivial branches: test stays with head, branches break to +2
            let header = Doc::concat([head, Doc::text(" "), test]);
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
                    Doc::text(" "),
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
        Doc::concat([head, Doc::text(" "), expr, Doc::HardBreak, clauses]).nest(1),
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
        let header = Doc::concat([head, Doc::text(" "), test]);
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
            Doc::concat([head, Doc::text(" "), test, Doc::HardBreak, body]).nest(1),
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
            Doc::concat([head, Doc::text(" "), test, Doc::Break, body])
                .nest(1)
                .group(),
            Doc::text(")"),
        ]))
    } else {
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([head, Doc::text(" "), test, Doc::HardBreak, body]).nest(1),
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
        Doc::concat([
            head,
            Doc::text(" "),
            item,
            Doc::text(" "),
            in_kw,
            Doc::text(" "),
            coll,
        ])
    } else {
        Doc::concat([head, Doc::text(" "), item, Doc::text(" "), coll])
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
    let mut pair_docs = Vec::new();
    let mut i = 0;
    while i < children.len() {
        let test = format_annotated(&children[i], source, config);
        i += 1;
        if i < children.len() {
            if is_trivial(&children[i]) {
                let result = format_annotated(&children[i], source, config);
                pair_docs.push(Doc::concat([test, Doc::text(" "), result]));
            } else {
                // Format body without trailing trivia inside the nest,
                // then append trailing trivia outside so comment breaks
                // don't inherit the nest indent.
                let body = format_without_trailing(&children[i], source, config);
                let trivia = format_trailing_trivia(&children[i]);
                pair_docs.push(Doc::concat([
                    test,
                    Doc::concat([Doc::HardBreak, body]).nest(1),
                    trivia,
                ]));
            }
            i += 1;
        } else {
            pair_docs.push(test);
        }
    }
    pair_docs
}
