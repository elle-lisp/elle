// audited: 2026-11-20
//! Tests for the epoch 18 rule: the forms it leaves, the forward reads the compiler then refuses, and the reads that are not forward.
//!
//! docs/epochs/bodies.md

use super::*;

/// The notes `elle rewrite` makes over `body`, written at epoch 17, after it
/// checks that the text keeps `body` as written.
fn notes_left(body: &str) -> Vec<String> {
    let source = format!("(elle/epoch 17)\n{body}\n");
    let rewritten = rewrite_file_with_notes(&source, "<test>").unwrap();
    let (text, _) = rewritten.change.expect("the epoch tag changes");
    assert_eq!(
        text,
        format!("(elle/epoch {CURRENT_EPOCH})\n{body}\n"),
        "the text rewrite"
    );
    rewritten.notes
}

/// The rule leaves `body`, written at epoch 17, as written, and its forward
/// read of `name` at `line` and `col` of `body`. `elle rewrite` names the
/// read, and the compiler refuses it.
fn refused(body: &str, name: &str, line: usize, col: usize) {
    let notes = notes_left(body);
    let read = format!("'{name}' is read before its def");
    // The rewritten file holds the epoch tag above the body.
    let at = format!("{}:{col}: ", line + 1);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with(&at) && n.contains(&read)),
        "a note names the read at {at}: {notes:?}"
    );
    let err = migrated_from(17, body).unwrap_err();
    assert!(err.contains(&read), "the compiler names the read: {err}");
    assert!(
        err.contains(&format!("<test>:{line}:{col}")),
        "the compiler names where the read stands: {err}"
    );
}

/// `body`, written at epoch 17, holds no forward read, so it stays as written
/// through both, with no note.
fn stays(body: &str) {
    assert!(notes_left(body).is_empty(), "no note for {body}");
    assert_eq!(
        migrated_from(17, body).unwrap(),
        printed(body),
        "the tree migration"
    );
}

// --- a def that cannot move, in a run a letrec cannot hold ---

#[test]
fn a_run_that_holds_an_expression_stays() {
    // A letrec binds only names, and an expression moved into its body would
    // run after every def of the run. `n` does not move past the expression,
    // and `g` is the body's last form.
    refused("(defn f [] n) (println 1) (def n 1) (f)", "n", 1, 12);
    refused("(defn f [] (g)) (println 1) (defn g [] 1)", "g", 1, 13);
}

#[test]
fn a_run_that_binds_a_name_twice_stays() {
    refused(
        "(defn f [] (g)) (def x 1) (def x 2) (defn g [] x) (f)",
        "g",
        1,
        13,
    );
}

#[test]
fn a_def_a_form_reads_directly_does_not_move_above_the_form() {
    // Up to epoch 17 the direct read found the binding outside the body; above
    // it the def would give that read its own value. In the letrec the read
    // would find a binding whose initializer has not run.
    refused("(defn f [] (g)) (def y (g)) (defn g [] 1) (f)", "g", 1, 13);
    refused(
        "(def x 1) (defn f [] (g)) (def x (+ x 1)) (defn g [] x) (f)",
        "g",
        1,
        23,
    );
}

#[test]
fn a_def_does_not_move_past_a_macro_it_uses() {
    // Above the macro's def, the call would no longer expand.
    refused(
        "(defn f [] (g)) (defmacro m [] 1) (defn g [] (m)) (f)",
        "g",
        1,
        13,
    );
}

#[test]
fn a_def_that_binds_a_pattern_does_not_move() {
    refused("(defn f [] (g)) (def [g] [(fn [] 1)]) (f)", "g", 1, 13);
}

#[test]
fn a_def_does_not_move_past_an_expression() {
    // Up to epoch 17 the expression ran before `g`'s def, and it may reach
    // `g`, or be a macro that binds a name `g` reads.
    refused("(defn f [] (g)) (println 1) (defn g [] 1) (f)", "g", 1, 13);
    refused("(map (fn [] (g)) [1]) (defn g [] 1) 2", "g", 1, 14);
    refused("(map (fn [] (g)) [1]) (defn g [] 1)", "g", 1, 14);
}

#[test]
fn every_forward_read_left_is_named() {
    let notes = notes_left("(defn f [] (g) (h)) (println 1) (def g 1) (def h 2) 3");
    assert_eq!(notes.len(), 2, "{notes:?}");
}

#[test]
fn a_def_that_reads_its_own_name_directly_and_in_a_lambda_stays() {
    // A let would hide the def's binding from the lambda; the compiler then
    // refuses the direct read as one before the binding's initialization.
    let body = "(def x 1) (def x [x (fn [] x)]) x";
    let notes = notes_left(body);
    assert!(
        notes.iter().any(|n| n.starts_with("2:11: ")),
        "a note names the def: {notes:?}"
    );
    assert_eq!(migrated_from(17, body).unwrap(), printed(body));
}

// --- a read that is not forward ---

#[test]
fn a_read_after_the_def_stays() {
    stays("(defn g [] 1) (defn f [] (g))");
    stays("(defn f [n] (f n))");
}

#[test]
fn a_name_the_reading_function_binds_again_is_not_a_forward_read() {
    stays("(defn f [g] (g)) (defn g [] 1)");
    stays("(defn f [] (let [g 1] g)) (defn g [] 2)");
    stays("(defn f [] (def g 1) g) (defn g [] 2)");
}

#[test]
fn a_quoted_name_is_not_a_read() {
    stays("(defn f [] 'g) (defn g [] 1)");
}

#[test]
fn a_direct_read_before_the_def_is_not_a_forward_read() {
    // It found the binding outside the body before epoch 18, and still does.
    stays("(defn f [] (def a b) (def b 1) a)");
}

#[test]
fn a_def_in_another_body_is_not_the_name_read() {
    stays("(defn f [] (g)) (defn h [] (defn g [] 1) (g))");
}
