// audited: 2026-11-24
//! A list that spells out a quoting form reads as the node its prefix builds.
//!
//! docs/impl/syntax.md

use super::*;

/// The form `text` reads as, with the byte offsets a span needs.
fn read(text: &str) -> Syntax {
    crate::reader::read_syntax(crate::syntax::thread_arena(), text, "<test>").unwrap()
}

#[test]
fn a_spelled_quoting_form_reads_as_its_prefix_node() {
    let pairs = [
        ("(quote x)", "'x"),
        ("(quasiquote x)", "`x"),
        ("(unquote x)", ",x"),
        ("(unquote-splicing x)", ",;x"),
    ];
    for (spelled, prefixed) in pairs {
        let node = read(spelled);
        assert_eq!(node.kind_label(), read(prefixed).kind_label(), "{spelled}");
        assert_eq!(node.to_string(), prefixed, "{spelled} prints as its prefix");
        // The formatter copies a node's source text by its span, so the span
        // covers the list the text wrote.
        assert_eq!(
            (node.span.start, node.span.end),
            (0, spelled.len() as u32),
            "{spelled}"
        );
    }
}

#[test]
fn a_list_that_spells_no_quoting_form_stays_a_list() {
    // A quoting form holds exactly one form under a bare symbol head.
    for text in [
        "(quote)",
        "(quote a b)",
        "(\"quote\" x)",
        "(quoted x)",
        "(:quote x)",
    ] {
        assert_eq!(read(text).kind_label(), "list", "{text}");
    }
}

#[test]
fn a_spelled_quoting_form_nests() {
    let node = read("(quasiquote (a (unquote (b (unquote-splicing c)))))");
    assert_eq!(node.to_string(), "`(a ,(b ,;c))");
}
