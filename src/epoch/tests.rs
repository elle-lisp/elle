// audited: 2026-09-23
//! Tests for the epoch declaration: extracting it, the prescan's frozen
//! grammar, and the check that the declaration chose the lexicon.
//!
//! docs/epochs.md
//! docs/impl/lexicon.md

use super::*;
use crate::syntax::{thread_arena, Span, Syntax, SyntaxKind};

fn sym(name: &str) -> Syntax {
    Syntax::symbol(&thread_arena(), name, Span::synthetic())
}

fn int(n: i64) -> Syntax {
    Syntax::new(SyntaxKind::Int(n), Span::synthetic())
}

fn list(items: Vec<Syntax>) -> Syntax {
    Syntax::list(&thread_arena(), &items, Span::synthetic())
}

#[test]
fn test_extract_epoch_present() {
    let mut forms = vec![
        list(vec![sym("elle/epoch"), int(0)]),
        list(vec![sym("def"), sym("x"), int(10)]),
    ];

    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, Some(0));
    assert_eq!(forms.len(), 1); // (elle/epoch 0) removed
}

#[test]
fn test_extract_epoch_absent() {
    let mut forms = vec![list(vec![sym("def"), sym("x"), int(10)])];

    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
    assert_eq!(forms.len(), 1); // unchanged
}

#[test]
fn test_extract_epoch_empty() {
    let mut forms: Vec<Syntax> = Vec::new();
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
}

#[test]
fn test_extract_epoch_negative() {
    let mut forms = vec![list(vec![sym("elle/epoch"), int(-1)])];
    let result = extract_epoch(&mut forms);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must be non-negative"));
}

#[test]
fn test_extract_epoch_future() {
    let mut forms = vec![list(vec![sym("elle/epoch"), int(CURRENT_EPOCH as i64 + 1)])];
    let result = extract_epoch(&mut forms);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("only supports up to"));
}

#[test]
fn test_extract_epoch_not_elle() {
    let mut forms = vec![list(vec![sym("notelle"), int(0)])];
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
    assert_eq!(forms.len(), 1);
}

#[test]
fn test_extract_epoch_wrong_arity() {
    let mut forms = vec![list(vec![sym("elle/epoch")])];
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None); // not recognized, left alone
}

#[test]
fn test_migrate_forms_current_epoch() {
    let mut forms = vec![list(vec![sym("foo"), int(1)])];
    let count = migrate_forms(&thread_arena(), &mut forms, CURRENT_EPOCH).unwrap();
    assert_eq!(count, 0);
}

// --- prescan_epoch: the frozen micro-grammar (docs/impl/lexicon.md) ---

#[test]
fn test_prescan_bare_declaration() {
    assert_eq!(prescan_epoch("(elle/epoch 3)\n(def x 1)").unwrap(), 3);
}

#[test]
fn test_prescan_after_shebang() {
    assert_eq!(
        prescan_epoch("#!/usr/bin/env elle\n(elle/epoch 5)\n(def x 1)").unwrap(),
        5
    );
}

#[test]
fn test_prescan_leading_whitespace() {
    assert_eq!(prescan_epoch("\n\n  (elle/epoch 0)").unwrap(), 0);
}

#[test]
fn test_prescan_inner_whitespace() {
    assert_eq!(prescan_epoch("( elle/epoch  7 )").unwrap(), 7);
}

#[test]
fn test_prescan_absent_targets_current() {
    assert_eq!(prescan_epoch("(def x 1)").unwrap(), CURRENT_EPOCH);
}

#[test]
fn test_prescan_empty_and_shebang_only() {
    assert_eq!(prescan_epoch("").unwrap(), CURRENT_EPOCH);
    assert_eq!(prescan_epoch("#!/usr/bin/env elle").unwrap(), CURRENT_EPOCH);
}

#[test]
fn test_prescan_declaration_below_comment_is_invisible() {
    // Comment syntax is epoch-dependent, so the prescan cannot skip a
    // comment without the answer it is computing. Skipping `#`-comments
    // here would have looked right — and silently pinned `#` as a comment
    // character in every future epoch.
    assert_eq!(prescan_epoch("# c\n(elle/epoch 3)").unwrap(), CURRENT_EPOCH);
}

#[test]
fn test_prescan_not_first_form() {
    assert_eq!(
        prescan_epoch("(def y 2) (elle/epoch 3)").unwrap(),
        CURRENT_EPOCH
    );
}

#[test]
fn test_prescan_too_new_is_an_error() {
    let err = prescan_epoch(&format!("(elle/epoch {})", CURRENT_EPOCH + 1)).unwrap_err();
    assert!(err.contains("only supports up to"));
    // An epoch too large for u64 is the same refusal, not a silent miss.
    let err = prescan_epoch("(elle/epoch 99999999999999999999)").unwrap_err();
    assert!(err.contains("only supports up to"));
}

#[test]
fn test_prescan_negative_is_not_a_match() {
    // Digits only: `-1` is no declaration to the prescan. extract_epoch
    // still rejects it on the parsed tree with "must be non-negative".
    assert_eq!(prescan_epoch("(elle/epoch -1)").unwrap(), CURRENT_EPOCH);
}

#[test]
fn test_prescan_malformed_is_not_a_match() {
    assert_eq!(prescan_epoch("(elle/epoch x)").unwrap(), CURRENT_EPOCH);
    assert_eq!(prescan_epoch("(elle/epoch 3 4)").unwrap(), CURRENT_EPOCH);
    assert_eq!(prescan_epoch("(elle/epoch 3").unwrap(), CURRENT_EPOCH);
    // The symbol must end exactly: a longer name is a different name.
    assert_eq!(prescan_epoch("(elle/epochs 3)").unwrap(), CURRENT_EPOCH);
    assert_eq!(prescan_epoch("(elle/epoch3)").unwrap(), CURRENT_EPOCH);
}

#[test]
fn epoch_13_is_the_first_epoch_whose_lexicon_moves() {
    // Epochs 0 to 12 read string escapes one way and epoch 13 another
    // (docs/epochs.md). A lexicon that moved at any other epoch would change
    // how an existing file tokenizes, with no entry in the history to say so.
    for epoch in 0..=12 {
        assert_eq!(
            rules::Lexicon::for_epoch(epoch),
            rules::Lexicon::for_epoch(0),
            "epoch {epoch}"
        );
    }
    assert_ne!(rules::Lexicon::for_epoch(13), rules::Lexicon::for_epoch(12));
}

// --- the mismatch check (docs/impl/lexicon.md) ---

/// The forms of `source`, read the way the pipeline reads them.
fn forms_of(source: &str) -> Vec<Syntax> {
    read_syntax_all(thread_arena(), source, "t.lisp").unwrap()
}

#[test]
fn a_declaration_below_a_comment_is_allowed_when_it_names_the_current_lexicon() {
    // The prescan cannot see this declaration and answers CURRENT_EPOCH. The
    // declaration selects the same lexicon, so the file was read the way its
    // author meant.
    let source = format!("# what this file is\n(elle/epoch {CURRENT_EPOCH})\n(def x 1)");
    assert_eq!(prescan_epoch(&source).unwrap(), CURRENT_EPOCH);
    check_lexicon_agreement(&forms_of(&source), &source, "t.lisp").unwrap();
}

#[test]
fn a_declaration_below_a_comment_is_refused_when_its_epoch_lexes_differently() {
    // The prescan cannot see this declaration, so the file was lexed under
    // the current rules. Epoch 12 reads string escapes by other rules, so a
    // `"\x41"` in this file would already hold a string its author never
    // wrote. This is the refusal reached through two registered epochs.
    let source = "# what this file is\n(elle/epoch 12)\n(def x 1)";
    let err = check_lexicon_agreement(&forms_of(source), source, "t.lisp").unwrap_err();
    assert!(err.contains("(elle/epoch 12)"), "{err}");
    assert!(err.contains("shebang"), "{err}");
}

#[test]
fn a_source_without_a_declaration_needs_no_agreement() {
    let source = "(def x 1)";
    check_lexicon_agreement(&forms_of(source), source, "t.lisp").unwrap();
}

#[test]
fn a_declaration_this_compiler_cannot_act_on_is_left_to_extract_epoch() {
    // A negative epoch is not a lexicon the check could look up. It stays
    // extract_epoch's error to report, so the check must pass it through
    // rather than panic looking the number up.
    let source = "(elle/epoch -1)\n(def x 1)";
    check_lexicon_agreement(&forms_of(source), source, "t.lisp").unwrap();
    assert!(extract_epoch(&mut forms_of(source)).is_err());
}

#[test]
fn agreeing_lexicons_accept_a_pair_of_different_epochs() {
    // Epochs 3 and 12 differ in number and share a lexicon. Comparing the
    // numbers would refuse this pair; the rule compares lexicons.
    refuse_mismatch(EpochLexicon::of(3), EpochLexicon::of(12), "t.lisp").unwrap();
}

#[test]
fn differing_lexicons_refuse_the_file_and_name_the_fix() {
    // `divergent` is a lexicon no registered epoch has, so this pins the
    // refusal whichever epochs happen to lex differently.
    let err = refuse_mismatch(
        EpochLexicon::with_lexicon(3, rules::Lexicon::divergent()),
        EpochLexicon::of(CURRENT_EPOCH),
        "t.lisp",
    )
    .unwrap_err();
    assert!(err.contains("t.lisp"), "{err}");
    assert!(err.contains("(elle/epoch 3)"), "{err}");
    assert!(err.contains(&format!("epoch {}", CURRENT_EPOCH)), "{err}");
    assert!(err.contains("shebang"), "{err}");
}
