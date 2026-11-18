// audited: 2026-11-18
//! Tests for the epoch 18 migration of the tree: its epoch, its count, the span of the letrec, and the refusal of a read it leaves.
//!
//! docs/epochs.md
//!
//! The cases the tree shares with `elle rewrite` live with the rewrite's tests,
//! which hold both to one answer. These are the tree's own.

use super::*;

const FORWARD: &str = "(defn f [] (g)) (defn g [] 1) (f)";

#[test]
fn the_rule_belongs_to_epoch_18() {
    assert_eq!(
        migrated_text(FORWARD, 16, 17).unwrap(),
        "(defn f [] (g))\n(defn g [] 1)\n(f)"
    );
    assert_eq!(
        migrated_text(FORWARD, 17, 18).unwrap(),
        "(letrec [f (fn [] (g)) g (fn [] 1)] (f))"
    );
}

#[test]
fn each_def_the_rule_moves_counts_one_rewrite() {
    // Two defs join a letrec, and one becomes a let. The range holds no other
    // rule, so the count is the rule's alone.
    let arena = thread_arena();
    let source = "(defn f [] (g)) (defn g [] 1) (def n 1) (def n (+ n 1)) n";
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    assert_eq!(migrate(&arena, &mut forms, 17, 18).unwrap(), 3);
}

#[test]
fn the_letrec_carries_the_span_of_its_first_def() {
    // An error in the letrec points where the author wrote the first def.
    let arena = thread_arena();
    let source = "(def a 1)\n(defn f [] (g))\n(defn g [] a)\n(f)";
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    migrate(&arena, &mut forms, 17, 18).unwrap();
    let the_letrec = forms[1];
    assert!(
        the_letrec.as_list().unwrap()[0].is_symbol("letrec"),
        "{the_letrec}"
    );
    assert_eq!((the_letrec.span.line, the_letrec.span.col), (2, 1));
}

#[test]
fn a_forward_read_the_rule_leaves_is_a_migration_error() {
    // At epoch 18 the read would find a binding outside the body, or none.
    let err = migrated_text("(defn f [] (g))\n(println 1)\n(defn g [] 1)", 17, 18).unwrap_err();
    assert!(err.contains("'g' is read before its def"), "{err}");
    assert!(err.contains("<test>:1:13"), "{err}");
}
