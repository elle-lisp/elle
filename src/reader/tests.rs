//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::epoch::rules::Lexicon;
use crate::epoch::CURRENT_EPOCH;

/// A declaration one epoch past what this compiler knows. No lexicon
/// exists for it, so every reader entry point must refuse the source.
fn too_new() -> String {
    format!("(elle/epoch {})", CURRENT_EPOCH + 1)
}

#[test]
fn a_declaration_from_the_future_stops_the_reader() {
    // The reader picks a lexicon before it tokenizes, and it has no rules
    // for an unregistered epoch. Without the prescan the text lexes fine
    // and only the pipeline's extract_epoch objects — long after the
    // reader produced tokens it had no rules to produce.
    let src = format!("{}\n(def x 1)", too_new());
    let err = read_syntax_all(crate::syntax::thread_arena(), &src, "t.lisp").unwrap_err();
    assert!(err.contains("only supports up to"), "{err}");
}

#[test]
fn a_shebang_does_not_hide_the_declaration_from_the_reader() {
    let src = format!("#!/usr/bin/env elle\n{}\n(def x 1)", too_new());
    let err = read_syntax_all(crate::syntax::thread_arena(), &src, "t.lisp").unwrap_err();
    assert!(err.contains("only supports up to"), "{err}");
}

#[test]
fn a_literate_document_prescans_the_stripped_text() {
    // Prose is not Elle: the declaration is the first form of the first
    // fence. Prescanning the raw markdown reaches the `#` heading first
    // and reports "no declaration", which reads the file under the wrong
    // lexicon instead of refusing it.
    let src = format!(
        "# Title\n\nProse.\n\n```lisp\n{}\n(def x 1)\n```\n",
        too_new()
    );
    let err = read_syntax_all_for(crate::syntax::thread_arena(), &src, "t.md").unwrap_err();
    assert!(err.contains("only supports up to"), "{err}");
}

#[test]
fn read_str_stops_on_a_declaration_from_the_future() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let mut symbols = SymbolTable::new();
    let src = format!("{}\n(def x 1)", too_new());
    let err = read_str(&src, &mut heap, &mut symbols).unwrap_err();
    assert!(err.contains("only supports up to"), "{err}");
}

#[test]
fn a_supported_declaration_reads_as_an_ordinary_form() {
    // The prescan only picks the lexicon. The declaration stays in the
    // tree for extract_epoch to consume.
    let forms = read_syntax_all(
        crate::syntax::thread_arena(),
        "(elle/epoch 3)\n(def x 1)",
        "t.lisp",
    )
    .unwrap();
    assert_eq!(forms.len(), 2);
}

#[test]
fn an_undeclared_source_reads_under_the_current_lexicon() {
    let forms = read_syntax_all(crate::syntax::thread_arena(), "# c\n(def x 1)", "t.lisp").unwrap();
    assert_eq!(forms.len(), 1);
}

#[test]
fn a_literate_document_reports_the_epoch_of_its_first_fence() {
    // The pipeline asks which epoch tokenized a file so it can compare that
    // answer against the declaration in the tree. For a literate document
    // the answer must come from the stripped text, as the lexer's did.
    let md = "# Title\n\nProse.\n\n```lisp\n(elle/epoch 3)\n(def x 1)\n```\n";
    assert_eq!(prescanned_epoch_for(md, "t.md").unwrap(), 3);
    // Prescanning the raw markdown reaches the heading and reports "none".
    assert_eq!(crate::epoch::prescan_epoch(md).unwrap(), CURRENT_EPOCH);
}

#[test]
fn a_plain_source_reports_the_epoch_it_declares() {
    assert_eq!(
        prescanned_epoch_for("(elle/epoch 3)\n(def x 1)", "t.lisp").unwrap(),
        3
    );
    assert_eq!(
        prescanned_epoch_for("(def x 1)", "t.lisp").unwrap(),
        CURRENT_EPOCH
    );
}

#[test]
fn lex_all_tokenizes_under_the_lexicon_it_is_given() {
    // The same three bytes, two token streams. This pins that the choice
    // reaches the tokens through lex_all's own body, not only through a
    // Lexer a test builds by hand.
    let current = lex_all(";xs", "t.lisp").unwrap();
    assert_eq!(
        current.tokens,
        vec![OwnedToken::Splice, OwnedToken::Symbol("xs".to_string())]
    );
    let divergent = lex_all_under(";xs", "t.lisp", Lexicon::divergent()).unwrap();
    assert_eq!(
        divergent.tokens,
        vec![OwnedToken::Comment(";xs".to_string())]
    );
}

#[test]
fn the_current_lexicon_entry_point_ignores_a_declaration() {
    // Prompt input is always current-epoch (docs/impl/lexicon.md). A pasted
    // declaration is an ordinary form there, not a choice of lexer, so the
    // text the prescanning entry point refuses must still read.
    let src = format!("{}\n(def x 1)", too_new());
    assert!(read_syntax_all(crate::syntax::thread_arena(), &src, "<repl>").is_err());
    let forms = read_syntax_all_current(crate::syntax::thread_arena(), &src, "<repl>").unwrap();
    assert_eq!(forms.len(), 2);
}

#[test]
fn the_shebang_length_is_the_bytes_the_lexer_never_sees() {
    // Four callers translate between original-source offsets and lexer
    // offsets with this number; they must all get the same one.
    assert_eq!(shebang_len("(def x 1)"), 0);
    assert_eq!(shebang_len("#!/usr/bin/env elle\n(def x 1)"), 20);
    // A shebang with no newline is the whole file.
    assert_eq!(shebang_len("#!/usr/bin/env elle"), 19);
}
