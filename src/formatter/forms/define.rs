// audited: 2026-10-27
//! Binding and definitional forms: `def`, `defn`, `fn`, `let`/`let*`/`letrec`,
//! `defmacro`.
//!
//! docs/fmt.md

use super::{format_body, format_generic_call, is_collection, is_string_literal, parens};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{
    format_annotated, format_leading_trivia, format_pair, format_trailing_trivia,
    format_without_trailing,
};
use crate::formatter::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

// ── def / defn ─────────────────────────────────────────────────

/// Format `(def name value)` or `(defn name [params] body...)`.
pub(in crate::formatter) fn format_def(
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
    parens(Doc::concat([
        head,
        Doc::Space,
        name,
        Doc::concat([Doc::Break, value]).group(),
    ]))
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

    parens(Doc::concat([
        header.group(),
        params_trivia,
        Doc::HardBreak,
        body,
    ]))
}

// ── fn / λ ─────────────────────────────────────────────────────

/// `(fn [params] body...)` or `(fn name [params] body...)`.
///
/// Inline if single short body expression; break otherwise.
pub(in crate::formatter) fn format_fn(
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

    // The header sits inside the Nest too, so a comment in it indents +2.
    if body_children.is_empty() {
        parens(header.group())
    } else if body_children.len() == 1 {
        // Single body: try inline, break if needed.
        let body_doc = format_annotated(&body_children[0], source, config);
        parens(Doc::concat([
            header.group(),
            params_trivia,
            Doc::concat([Doc::Break, body_doc]).group(),
        ]))
    } else {
        // Multiple body expressions: always break.
        let body = format_body(body_children, source, config);
        parens(Doc::concat([
            header.group(),
            params_trivia,
            Doc::HardBreak,
            body,
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
pub(in crate::formatter) fn format_let(
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
    let bindings_trivia = format_trailing_trivia(&children[1]);

    // Header: (let [...])
    let header = Doc::concat([head, Doc::Space, bindings_doc]);

    // Body: +2 indent
    let body = format_body(&children[2..], source, config);

    parens(Doc::concat([header, bindings_trivia, Doc::HardBreak, body]))
}

/// Format a binding vector, one pair per line, always, with the comments and
/// blank lines above it. The caller places its trailing trivia.
///
/// Uses Align after `[` so that subsequent binding names line up with
/// the first binding name regardless of nesting depth.
pub(super) fn format_bindings(
    bindings_node: &AnnotatedSyntax,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let items = &bindings_node.children;
    let leading = format_leading_trivia(bindings_node);

    if items.is_empty() {
        return Doc::concat([leading, Doc::text("[]")]);
    }

    // A name and its value share a line, never a Break, unless a comment comes
    // between them.
    let pairs = items.chunks(2).map(|pair| match pair {
        [name, value] => format_pair(name, value, source, config),
        [name] => format_annotated(name, source, config),
        _ => unreachable!("chunks(2) yields one or two"),
    });

    Doc::concat([
        leading,
        Doc::text("["),
        Doc::align(Doc::join_hardbreak(pairs)),
        Doc::text("]"),
    ])
}

// ── defmacro ───────────────────────────────────────────────────

/// `(defmacro name [params] body...)` — same layout as defn.
pub(in crate::formatter) fn format_defmacro(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    format_defn(children, source, config)
}
