// audited: 2026-10-06
// docs/impl/wasm.md
//! An error parks every frame from the raise to the fiber body on this tier,
//! so a restart answers the raising call inside a callee.
//!
//! The counter-factual is an error that returns through the status word
//! instead of parking: the callee's frame is gone, and the restart answers the
//! body's call to it. tests/lang/fiber-restart.lisp holds the same cases for
//! the interpreter.
//!
//! Each expression evaluates to a boolean so the assertion reads the
//! language's answer rather than a value's display form.

use super::*;

/// A callee's `(error …)`, its primitive raise, and a frame between it and the
/// body.
#[test]
fn wasm_full_restart_answers_the_raise_inside_a_callee() {
    for (src, why) in [
        (
            "(defn raise-now [] (+ 100 (error :boom))) \
             (let [f (fiber/new (fn [] (list :got (raise-now))) |:error|)] \
               (fiber/resume f) \
               (= (list :got 141) (fiber/resume f 41)))",
            "the restart must answer the error inside the callee",
        ),
        (
            "(defn get-now [] (+ 100 (get nil :x))) \
             (let [f (fiber/new (fn [] (list :got (get-now))) |:error|)] \
               (fiber/resume f) \
               (= (list :got 141) (fiber/resume f 41)))",
            "the restart must answer the primitive's raise inside the callee",
        ),
        (
            "(defn raise-now [] (+ 100 (error :boom))) \
             (defn outer-call [] (+ 1000 (raise-now))) \
             (let [f (fiber/new (fn [] (list :got (outer-call))) |:error|)] \
               (fiber/resume f) \
               (= (list :got 1141) (fiber/resume f 41)))",
            "every frame between the raise and the body must run on",
        ),
        (
            "(defn raise-now [] (+ 100 (error :boom))) \
             (defn via-tail [] (raise-now)) \
             (let [f (fiber/new (fn [] (list :got (via-tail))) |:error|)] \
               (fiber/resume f) \
               (= (list :got 141) (fiber/resume f 41)))",
            "a tail call must park the callee it entered",
        ),
    ] {
        assert_eq!(eval_with_stdlib(src), "true", "{why}: {src}");
    }
}

/// An `:error` fiber, whose mask lets the error pass to the resumer, restarts
/// inside the callee too.
#[test]
fn wasm_full_an_error_fiber_restarts_inside_the_callee() {
    assert_eq!(
        eval_with_stdlib(
            "(defn get-now [] (+ 100 (get nil :x))) \
             (let [f (fiber/new (fn [] (list :got (get-now))) |:yield|)] \
               (protect (fiber/resume f)) \
               (and (= :error (fiber/status f)) \
                    (= (list :got 141) (fiber/resume f 41))))"
        ),
        "true",
        "a restart of an :error fiber must answer the raise inside the callee"
    );
}
