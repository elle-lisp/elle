// audited: 2026-11-13
//! A lambda `def` nested in a file's top-level form settles with the file's other signals, hoisted or merged.
//!
//! docs/pipeline.md

use super::*;

// Counter-factual: a lambda a `def` binds inside a top-level form read the
// silent seed of the later `g`. Left at that seed, `f` stayed silent, and a
// caller trusting it broke its proof at run time.
#[test]
fn a_lambda_def_nested_in_a_top_level_form_takes_its_callee_s_settled_signal() {
    let result = analyze_file("(def k (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    assert_eq!(signal_of(&result, "g"), Signal::yields());
    assert_eq!(
        signal_of(&result, "f"),
        Signal::yields(),
        "f yields as g does"
    );
}

#[test]
fn a_caller_of_a_nested_lambda_def_takes_its_settled_signal() {
    let result =
        analyze_file("(def k (do (defn f [] (g)) 1)) (defn h [] (f)) (defn g [] (yield 1))");
    assert_eq!(
        signal_of(&result, "h"),
        Signal::yields(),
        "h yields as f does"
    );
}

#[test]
fn a_lambda_def_nested_in_a_top_level_expression_settles_too() {
    let result = analyze_file("(identity (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    assert_eq!(
        signal_of(&result, "f"),
        Signal::yields(),
        "f yields as g does"
    );
}

// Counter-factual: `early` reads `f` before its nested `def`, so the file sets
// `f` first and the `def` assigns it. A `def` that assigns rather than defines
// its binding kept `g`'s silent seed for `f`, and its call to `g` broke its
// proof at run time.
#[test]
fn a_hoisted_lambda_def_nested_in_a_top_level_form_settles_too() {
    let result =
        analyze_file("(defn early [] (f)) (def k (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    let [_early, f, _g] = lambda_signals(&result.hir)[..] else {
        panic!("expected early, f and g");
    };
    assert_eq!(f, Signal::yields(), "f yields as g does");
}

// Counter-factual: the merged bindings the file's last form leaves waiting
// hid the move of `a`'s signal, so `h` kept the silence it read in the pass.
#[test]
fn a_merge_the_last_form_leaves_hides_no_signal_that_moved() {
    let result = analyze_file(
        "(defn a [] (b) :a)
         (defn h [] (a) :h)
         (defn b [] (yield 1))
         (def c true)
         (when c (defn f [] 1))",
    );
    assert_eq!(signal_of(&result, "a"), Signal::yields());
    assert_eq!(
        signal_of(&result, "h"),
        Signal::yields(),
        "h yields as a does"
    );
}
