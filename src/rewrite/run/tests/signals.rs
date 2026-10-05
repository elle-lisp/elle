// audited: 2026-10-05
//! Tests for the epoch 15 rewrite of real source text: each signal concept
//! takes one form, and squelch takes its spec first.
//!
//! docs/epochs.md

use super::*;

/// `body` written at epoch 14, the last epoch with `silence`, rewritten.
fn rewritten_from_14(body: &str) -> String {
    rewritten_from(14, body)
}

#[test]
fn silence_takes_the_form_of_its_arity() {
    assert_eq!(
        rewritten_from_14("(defn f [] (silence) 1)\n"),
        "(defn f [] (silent!) 1)\n"
    );
    assert_eq!(
        rewritten_from_14("(defn f [g] (silence g) (g))\n"),
        "(defn f [g] (bound g ||) (g))\n"
    );
}

#[test]
fn squelch_takes_its_spec_first() {
    assert_eq!(
        rewritten_from_14("(def q (squelch p :yield))\n"),
        "(def q (squelch :yield p))\n"
    );
    assert_eq!(
        rewritten_from_14("(squelch (fn [] (yield 1)) |:yield :io|)\n"),
        "(squelch |:yield :io| (fn [] (yield 1)))\n"
    );
}

#[test]
fn a_squelch_nested_in_an_argument_is_flipped_too() {
    // The trap in a replacement built from source text: the inner squelch
    // rides along inside the outer one's argument, unrewritten, and the file
    // keeps the order the epoch removed.
    assert_eq!(
        rewritten_from_14("(squelch (squelch f :yield) :io)\n"),
        "(squelch :io (squelch :yield f))\n"
    );
}

#[test]
fn a_rename_inside_a_squelch_argument_is_kept() {
    // The rename's edit falls inside the replaced form, and the structural
    // filter drops it. The argument text must carry it instead.
    assert_eq!(
        rewritten_from(9, "(squelch (fn [] (car x)) :yield)\n"),
        "(squelch :yield (fn [] (first x)))\n"
    );
}

#[test]
fn muffle_is_refused_with_what_to_write() {
    let source = "(elle/epoch 14)\n(defn f [] (muffle :yield) (yield 1))\n";
    let err = rewrite_file(source, "<test>").unwrap_err();
    assert!(err.contains("muffle"), "{err}");
    assert!(err.contains("(squelch spec f)"), "{err}");
}
