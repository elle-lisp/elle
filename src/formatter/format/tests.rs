// audited: 2026-09-23
//! Tests for the doc generator's fallback: the text it prints for a node
//! with no source text to copy.
//!
//! docs/fmt.md
//! docs/impl/lexicon.md

use super::*;
use crate::epoch::CURRENT_EPOCH;
use crate::syntax::{Span, Syntax, SyntaxHeap};

/// A string holding each character a printer can get wrong: a non-ASCII
/// character, both quotes, a backslash, and control characters with and
/// without an escape of their own.
const AWKWARD: &str = "é'\"\\\n\t\r\u{1}\0";

/// The text the formatter prints for `syntax` alone, with no source to copy.
fn printed(syntax: Syntax) -> String {
    let node = AnnotatedSyntax {
        syntax,
        leading: Vec::new(),
        trailing: Vec::new(),
        children: Vec::new(),
    };
    let config = FormatterConfig::default();
    super::super::render::render(&format_forms(&[node], &[], "", &config), &config)
}

#[test]
fn a_string_with_no_source_text_prints_a_literal_every_lexicon_reads_back() {
    // A span outside the source sends a string node to the fallback. It used
    // Rust's `escape_default`, which writes `\u{e9}` for `é` and `\'` for
    // `'`: epoch 12 reads the first as `u{e9}`, and epoch 13 refuses the
    // second.
    let (_home, a) = SyntaxHeap::with_arena();
    let text = printed(Syntax::string(&a, AWKWARD, Span::synthetic()));
    for epoch in [12, CURRENT_EPOCH] {
        let source = format!("(elle/epoch {epoch})\n{text}");
        let forms = crate::reader::read_syntax_all(a, &source, "<fallback>").unwrap();
        assert!(
            matches!(&forms[1].kind, SyntaxKind::String(s) if *s == AWKWARD),
            "epoch {epoch} read {text:?} as {:?}",
            forms[1].kind
        );
    }
}

#[test]
fn a_mutable_string_with_no_source_text_keeps_its_prefix() {
    let (_home, a) = SyntaxHeap::with_arena();
    assert_eq!(
        printed(Syntax::string_mut(&a, "é'", Span::synthetic())),
        "@\"é'\""
    );
}
