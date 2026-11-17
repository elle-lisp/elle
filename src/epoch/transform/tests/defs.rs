// audited: 2026-11-17
//! Tests for the epoch 17 migration of the tree: a def outside a body moves into a let.
//!
//! docs/epochs.md
//!
//! The cases the tree shares with `elle rewrite` live with the rewrite's tests,
//! which hold both to one answer. These are the tree's own.

use super::*;

const MOVED: &str = "(defn f [c] (when c (def x 1) (g x)))";

#[test]
fn the_rule_belongs_to_epoch_17() {
    assert_eq!(migrated_text(MOVED, 15, 16).unwrap(), MOVED);
    assert_eq!(
        migrated_text(MOVED, 16, 17).unwrap(),
        "(defn f [c] (when c (let [x 1] (g x))))"
    );
}

#[test]
fn each_def_the_rule_moves_counts_one_rewrite() {
    // The range holds no other rule, so the count is the rule's alone. A def
    // the rule leaves counts nothing.
    let arena = thread_arena();
    let source = "(defn f [c] (when c (def x 1) (g x)) (if c (def y 1) 2))\n\
                  (defn h [c] (when c (def z 1)) z)";
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    assert_eq!(migrate(&arena, &mut forms, 16, 17).unwrap(), 2);
}

#[test]
fn the_let_carries_the_span_of_the_def_it_replaces() {
    // An error in the let points where the author wrote the def.
    let arena = thread_arena();
    let source = "(defn f [c]\n  (when c (def x 1) (g x)))";
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    migrate(&arena, &mut forms, 16, 17).unwrap();
    let when = forms[0].as_list().unwrap()[3];
    let the_let = when.as_list().unwrap()[2];
    assert!(the_let.as_list().unwrap()[0].is_symbol("let"), "{the_let}");
    assert_eq!((the_let.span.line, the_let.span.col), (2, 11));
}
