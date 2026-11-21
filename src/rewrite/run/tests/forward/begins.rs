// audited: 2026-11-20
//! Tests for the epoch 18 rule: a begin directly in a body splices first when it holds a form of a run or a def that reads its own name.
//!
//! docs/epochs/bodies.md
//!
//! The counter-factual read each such begin as one form that is not a def, so
//! a def in it stayed, and the compiler refused the read.

use super::*;

#[test]
fn functions_that_call_each_other_in_a_begin_bind_in_one_letrec() {
    carries(
        "(fn [] (begin (def ev? (fn [n] (if (= n 0) true (od? (- n 1))))) \
         (def od? (fn [n] (if (= n 0) false (ev? (- n 1))))) (ev? 8)))",
        "(fn [] (letrec [ev? (fn [n] (if (= n 0) true (od? (- n 1)))) \
         od? (fn [n] (if (= n 0) false (ev? (- n 1))))] (ev? 8)))",
    );
}

#[test]
fn a_def_in_a_begin_moves_above_the_read() {
    carries(
        "(defn f [] (g)) (begin (defn g [] 1)) (f)",
        "(defn g [] 1) (defn f [] (g)) (f)",
    );
    carries(
        "(defn outer [] (begin (defn f [] (g)) (defn g [] 1)) (f))",
        "(defn outer [] (defn g [] 1) (defn f [] (g)) (f))",
    );
}

#[test]
fn a_read_in_a_begin_splices_its_begin() {
    // The counter-factual moved `g` above the begin and left the begin.
    carries(
        "(begin (defn f [] (g))) (defn g [] 1) (f)",
        "(defn g [] 1) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_begin_directly_in_a_spliced_begin_splices_too() {
    carries(
        "(defn f [] (g)) (begin (do (defn g [] 1))) (f)",
        "(defn g [] 1) (defn f [] (g)) (f)",
    );
}

#[test]
fn every_form_of_a_spliced_begin_takes_its_place() {
    carries(
        "(defn f [] (g)) (begin (defn g [] 1) (def k 2)) (f)",
        "(defn g [] 1) (defn f [] (g)) (def k 2) (f)",
    );
}

#[test]
fn a_def_that_reads_its_own_name_in_a_begin_splices_it() {
    // The counter-factual left the def in the begin, with a note.
    carries(
        "(fn [x] (begin (def x (+ x 1))) x)",
        "(fn [x] (let [x (+ x 1)] x))",
    );
}

#[test]
fn a_begin_that_holds_no_form_of_a_run_stays() {
    carries(
        "(begin (def a 1)) (defn f [] (g)) (defn g [] a) (f)",
        "(begin (def a 1)) (defn g [] a) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_spliced_run_the_rule_then_leaves_is_refused() {
    // The splice keeps the meaning, so it stands, and the read is named.
    let source = "(elle/epoch 17)\n(defn f [] (g)) (begin (println 1) (defn g [] 1)) (f)\n";
    let rewritten = rewrite_file_with_notes(source, "<test>").unwrap();
    let (text, _) = rewritten.change.expect("the epoch tag changes");
    assert_eq!(
        text,
        format!("(elle/epoch {CURRENT_EPOCH})\n(defn f [] (g)) (println 1) (defn g [] 1) (f)\n")
    );
    assert!(
        rewritten
            .notes
            .iter()
            .any(|n| n.starts_with("2:13: ") && n.contains("'g' is read before its def")),
        "a note names the read: {:?}",
        rewritten.notes
    );
    let err =
        migrated_from(17, "(defn f [] (g)) (begin (println 1) (defn g [] 1)) (f)").unwrap_err();
    assert!(err.contains("'g' is read before its def"), "{err}");
}

// --- the text alone ---

#[test]
fn a_begin_on_lines_of_its_own_leaves_its_forms_on_theirs() {
    assert_eq!(
        rewritten_from(17, "(defn f [] (g))\n(begin\n  (defn g [] 1))\n(f)\n"),
        "(defn g [] 1)\n(defn f [] (g))\n(f)\n"
    );
}

#[test]
fn a_comment_after_the_begin_head_stays() {
    assert_eq!(
        rewritten_from(17, "(defn f [] (g))\n(begin # g\n  (defn g [] 1))\n(f)\n"),
        "# g\n  (defn g [] 1)\n(defn f [] (g))\n(f)\n"
    );
}
