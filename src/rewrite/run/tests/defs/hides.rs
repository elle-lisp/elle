// audited: 2026-11-20
//! Tests for the epoch 17 rule: a read inside a prelude macro that binds the name again holds no def back.
//!
//! docs/epochs/bodies.md
//!
//! The counter-factual read each macro as a call, counted the name it binds
//! as a read of the def, and left the def as written.

use super::*;

#[test]
fn an_if_let_name_hides_the_read_in_its_then_and_its_else() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (if-let [x (h)] x x))",
        "(defn f [c] (when c (let [x 1] (g x))) (if-let [x (h)] x x))",
    );
}

#[test]
fn a_later_if_let_name_does_not_hide_the_else() {
    // When x binds nil, the else runs outside y's binding and reads the def.
    stays("(defn f [c] (when c (def y 1)) (if-let [x (h) y (k)] y y))");
}

#[test]
fn a_when_let_name_hides_the_read_in_its_body() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (when-let [x (h)] (k x)))",
        "(defn f [c] (when c (let [x 1] (g x))) (when-let [x (h)] (k x)))",
    );
}

#[test]
fn a_when_ok_or_with_name_hides_the_read_in_its_body() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (when-ok [x (h)] (k x)))",
        "(defn f [c] (when c (let [x 1] (g x))) (when-ok [x (h)] (k x)))",
    );
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (with x (h) close (k x)))",
        "(defn f [c] (when c (let [x 1] (g x))) (with x (h) close (k x)))",
    );
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (with-temp-dir x (k x)))",
        "(defn f [c] (when c (let [x 1] (g x))) (with-temp-dir x (k x)))",
    );
}

#[test]
fn the_value_a_binding_macro_binds_still_reads_the_def() {
    stays("(defn f [c] (when c (def x 1)) (when-let [y x] y))");
    stays("(defn f [c] (when c (def x 1)) (when-ok [y x] y))");
    stays("(defn f [c] (when c (def x 1)) (with y x close y))");
}
