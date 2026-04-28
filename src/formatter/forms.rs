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

use super::config::FormatterConfig;
use super::doc::Doc;
use super::format::{format_annotated, format_trailing_trivia, format_without_trailing};
use super::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

// ── Helpers ────────────────────────────────────────────────────

/// Check if a node is a string literal (for docstring detection).
fn is_string_literal(node: &AnnotatedSyntax) -> bool {
    matches!(node.syntax.kind, SyntaxKind::String(_))
}

/// Check if a node is a collection type (List, Array, etc.).
fn is_collection(node: &AnnotatedSyntax) -> bool {
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
fn is_trivial(node: &AnnotatedSyntax) -> bool {
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
        | SyntaxKind::String(_) => true,

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

/// Format a sequence of body expressions separated by HardBreaks.
///
/// CommentBreak (emitted after trailing comments by format_annotated)
/// is absorbed by the inter-sibling HardBreak, so no special-casing needed.
fn format_body(children: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Doc {
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

// ── def / defn ─────────────────────────────────────────────────

/// Format `(def name value)` or `(defn name [params] body...)`.
pub(super) fn format_def(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() >= 4 && is_collection(&children[2]) {
        format_defn(children, source, config)
    } else {
        format_def_simple(children, source, config)
    }
}

/// `(def name value)` — name on same line as def, value breaks with +2 if needed.
fn format_def_simple(children: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let name = format_annotated(&children[1], source, config);
    let value = format_annotated(&children[2], source, config);

    // (def name value) inline if fits, else (def name\n  value)
    Doc::concat([
        Doc::text("("),
        head,
        Doc::text(" "),
        name,
        Doc::concat([Doc::Break, value]).nest(1).group(),
        Doc::text(")"),
    ])
}

/// `(defn name [params] body...)` — always break before body.
///
/// ```lisp
/// (defn name [params]
///   body)
/// ```
fn format_defn(children: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Doc {
    // children: [defn, name, [params], body...]
    // or:       [defn, name, [params], "docstring", body...]
    if children.len() < 4 {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let name = format_annotated(&children[1], source, config);
    // Format params without trailing trivia so comments/blank-lines between
    // params and body don't poison the header group's measure_flat.
    let params = format_without_trailing(&children[2], source, config);
    let params_trivia = format_trailing_trivia(&children[2]);

    // Header: (defn name [params])
    let header = Doc::concat([head, Doc::Break, name, Doc::Break, params]);

    // Check for docstring (first body element is a string literal)
    let (docstring, body_start) = if children.len() > 3 && is_string_literal(&children[3]) {
        (Some(&children[3]), 4)
    } else {
        (None, 3)
    };

    // Build body: docstring (if present) + body expressions, all separated by HardBreaks
    let body = if let Some(ds_node) = docstring {
        let ds = format_annotated(ds_node, source, config);
        let rest = format_body(&children[body_start..], source, config);
        if children[body_start..].is_empty() {
            ds
        } else {
            Doc::concat([ds, Doc::HardBreak, rest])
        }
    } else {
        format_body(&children[body_start..], source, config)
    };

    Doc::concat([
        Doc::text("("),
        Doc::concat([header.group(), params_trivia, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ])
}

// ── fn / λ ─────────────────────────────────────────────────────

/// `(fn [params] body...)` or `(fn name [params] body...)`.
///
/// Inline if single short body expression; break otherwise.
pub(super) fn format_fn(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    // fn can have an optional name: (fn name [params] body) or (fn [params] body)
    let has_name = !is_collection(&children[1]);
    let params_idx = if has_name { 2 } else { 1 };
    let body_start = params_idx + 1;

    if children.len() < body_start {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    // Format params without trailing trivia so comments/blank-lines between
    // params and body don't poison the header group's measure_flat.
    let params = format_without_trailing(&children[params_idx], source, config);
    let params_trivia = format_trailing_trivia(&children[params_idx]);

    // Header: (fn name? [params])
    let mut header_parts = vec![head];
    if has_name {
        header_parts.push(Doc::Break);
        header_parts.push(format_annotated(&children[1], source, config));
    }
    header_parts.push(Doc::Break);
    header_parts.push(params);
    let header = Doc::concat(header_parts);

    let body_children = &children[body_start..];

    if body_children.is_empty() {
        // No body — just header
        Doc::concat([Doc::text("("), header.group(), Doc::text(")")])
    } else if body_children.len() == 1 {
        // Single body: try inline, break if needed.
        // Align so the body indents relative to (fn's column, not Nest level.
        let body_doc = format_annotated(&body_children[0], source, config);
        Doc::align(Doc::concat([
            Doc::text("("),
            header.group(),
            params_trivia,
            Doc::concat([Doc::Break, body_doc]).nest(1).group(),
            Doc::text(")"),
        ]))
    } else {
        // Multiple body expressions: always break.
        // Align so body indents relative to (fn's column.
        let body = format_body(body_children, source, config);
        Doc::align(Doc::concat([
            Doc::text("("),
            Doc::concat([header.group(), params_trivia, Doc::HardBreak, body]).nest(1),
            Doc::text(")"),
        ]))
    }
}

// ── let / letrec ───────────────────────────────────────────────

/// `(let [bindings...] body...)` — one binding pair per line, always.
///
/// ```lisp
/// (let [x 5
///       y 10]
///   body)
/// ```
pub(super) fn format_let(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < 3 {
        return format_generic_call(children, source, config);
    }

    // Only apply let-specific formatting when the bindings position is
    // actually a bracket form.  Inside quasiquotes the "bindings" slot
    // can be an unquote (`,more`) which must not be wrapped in brackets.
    if !matches!(children[1].syntax.kind, SyntaxKind::Array(_)) {
        return format_generic_call(children, source, config);
    }

    let head = format_annotated(&children[0], source, config);
    let bindings_doc = format_bindings(&children[1], source, config);

    // Header: (let [...])
    let header = Doc::concat([head, Doc::text(" "), bindings_doc]);

    // Body: +2 indent
    let body = format_body(&children[2..], source, config);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([header, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ]))
}

/// Format binding vector: one pair per line, always.
///
/// Uses Align after `[` so that subsequent binding names line up with
/// the first binding name regardless of nesting depth.
fn format_bindings(bindings_node: &AnnotatedSyntax, source: &str, config: &FormatterConfig) -> Doc {
    let items = &bindings_node.children;

    if items.is_empty() {
        return Doc::text("[]");
    }

    let mut pair_parts = Vec::new();
    let mut i = 0;
    let mut first = true;
    while i < items.len() {
        if !first {
            pair_parts.push(Doc::HardBreak);
        }
        first = false;

        // Name
        pair_parts.push(format_annotated(&items[i], source, config));
        i += 1;

        // Value (if present) — always a space, never a Break
        if i < items.len() {
            pair_parts.push(Doc::text(" "));
            pair_parts.push(format_annotated(&items[i], source, config));
            i += 1;
        }
    }

    Doc::concat([
        Doc::text("["),
        Doc::align(Doc::concat(pair_parts)),
        Doc::text("]"),
    ])
}

// ── if ─────────────────────────────────────────────────────────

/// `(if test then else?)`.
///
/// Inline if fits. When breaking:
/// - Trivial branches: columnar (align to first arg).
/// - Compound branches: +2 body indent.
pub(super) fn format_if(
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
pub(super) fn format_cond(
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
pub(super) fn format_match(
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
pub(super) fn format_while(
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

// ── defmacro ───────────────────────────────────────────────────

/// `(defmacro name [params] body...)` — same layout as defn.
pub(super) fn format_defmacro(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    format_defn(children, source, config)
}

// ── begin ──────────────────────────────────────────────────────

/// `(begin body...)` — always break. Each expression on its own line, +2 indent.
pub(super) fn format_begin(
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
pub(super) fn format_forever(
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
pub(super) fn format_block(
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
pub(super) fn format_parameterize(
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

    let body = format_body(&children[2..], source, config);

    Doc::align(Doc::concat([
        Doc::text("("),
        Doc::concat([head, Doc::text(" "), bindings, Doc::HardBreak, body]).nest(1),
        Doc::text(")"),
    ]))
}

// ── Threading macros ─────────────────────────────────────────

/// `(-> val step...)` — always break. Steps align with value.
pub(super) fn format_threading(
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

// ── when / unless ──────────────────────────────────────────────

/// `(when test body...)` or `(unless test body...)`.
///
/// Trivial body (single, no nested body forms): columnar alignment.
/// Compound body: +2 indent.
pub(super) fn format_when(
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
pub(super) fn format_each(
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
pub(super) fn format_case(
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
            let result = format_annotated(&children[i], source, config);
            if is_trivial(&children[i]) {
                pair_docs.push(Doc::concat([test, Doc::text(" "), result]));
            } else {
                pair_docs.push(Doc::concat([
                    test,
                    Doc::concat([Doc::HardBreak, result]).nest(1),
                ]));
            }
            i += 1;
        } else {
            pair_docs.push(test);
        }
    }
    pair_docs
}

// ── try / protect ──────────────────────────────────────────────

/// `(try body (catch pattern handler))` or `(protect body (finally cleanup))`.
///
/// Single short body: try inline. Multiple or long body: break.
pub(super) fn format_try(
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
pub(super) fn format_assign(
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

// ── Generic call (default) ────────────────────────────────────

/// Generic function call: try inline; break with args aligned to first arg.
///
/// Head and first arg stay on the same line. When breaking, subsequent
/// args align to the first arg's column. Keyword-value pairs (`:key val`)
/// are kept together as units so they break as one.
///
/// ```lisp
/// (f a b c)          # fits on one line
/// (f a               # doesn't fit — first arg stays with head
///    b               #   remaining args align to first arg
///    c)
/// ```
pub(super) fn format_generic_call(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.is_empty() {
        return Doc::text("()");
    }

    if children.len() == 1 {
        // Head only
        return Doc::concat([
            Doc::text("("),
            format_annotated(&children[0], source, config),
            Doc::text(")"),
        ]);
    }

    if children.len() == 2 {
        // Head + one arg: Align so arg indents to first-arg column
        let head = format_annotated(&children[0], source, config);
        let arg = format_annotated(&children[1], source, config);
        return Doc::concat([
            Doc::text("("),
            head,
            Doc::text(" "),
            Doc::align(Doc::concat([arg]).group()),
            Doc::text(")"),
        ]);
    }

    let head = format_annotated(&children[0], source, config);

    // Build arg units: keyword-value pairs are joined with a space,
    // positional args stand alone.
    let arg_units = build_arg_units(&children[1..], source, config);

    // Head + first arg always share the opening line.
    // Remaining args fill-wrap with +2 body indent.
    let first = arg_units[0].clone();
    let rest = &arg_units[1..];

    let opening = Doc::concat([head, Doc::text(" "), first]);

    if rest.is_empty() {
        Doc::concat([Doc::text("("), opening, Doc::text(")")])
    } else {
        let mut parts: Vec<Doc> = Vec::new();
        parts.push(opening);
        parts.extend(rest.iter().cloned());

        Doc::concat([Doc::text("("), Doc::fill(parts).nest(1), Doc::text(")")])
    }
}

/// Build argument units for generic calls, grouping `:keyword value` pairs.
///
/// A keyword followed by a non-keyword argument forms a single doc unit
/// joined by a space. Consecutive keywords or trailing keywords stand alone.
fn build_arg_units(args: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Vec<Doc> {
    let mut units = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let doc = format_annotated(&args[i], source, config);
        if matches!(args[i].syntax.kind, SyntaxKind::Keyword(_)) {
            // Keyword — pair with next arg if it's not also a keyword
            if i + 1 < args.len() && !matches!(args[i + 1].syntax.kind, SyntaxKind::Keyword(_)) {
                let val = format_annotated(&args[i + 1], source, config);
                units.push(Doc::concat([doc, Doc::text(" "), val]));
                i += 2;
                continue;
            }
        }
        units.push(doc);
        i += 1;
    }
    units
}
