// audited: 2026-09-29
// The `compile/*-module` queries run a test setup module on the calling fiber,
// and each names itself in the error that setup raises.
//
// docs/test-runner.md

use crate::common::eval_source;

/// The message of the error `query` raises for a setup module whose `def`
/// initializer yields. The setup run cannot hold that park, so it refuses it
/// and raises `:barrier-error` at the query's own call. Only
/// `compile/barrier-module` runs a `def` in its setup; `compile/whole-module`
/// wraps every form in the thunk it returns.
fn refusal_message(query: &str) -> String {
    let src = format!(
        r#"(let [[ok? err] (protect ({query} "(def x (yield 1))" "<setup>"))]
             (assert (not ok?) "a yielding setup raises")
             (assert (= (get err :error) :barrier-error) "the error is a barrier error")
             (get err :message))"#
    );
    eval_source(&src, |r| {
        let v = r.unwrap_or_else(|e| panic!("{query}: {e}"));
        v.with_string(|s| s.to_string())
            .unwrap_or_else(|| panic!("{query}: the message is not a string"))
    })
}

// The counter-factual is one label for every caller: the message then names
// `compile/whole-module-syntax` for a query the file never called.
#[test]
fn a_barrier_module_setup_refusal_names_compile_barrier_module() {
    let msg = refusal_message("compile/barrier-module");
    assert!(msg.starts_with("compile/barrier-module:"), "{msg}");
}
