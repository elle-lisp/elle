// audited: 2026-10-04
// An uncaught raise reports as an ordinary error whatever the raiser's
// inferred signal: exit 1, never an abort.
//
// docs/signals/inference.md

use std::process::Command;

/// Run `elle -e source` and answer its exit code and what it wrote to stderr.
fn run(source: &str) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["-e", source])
        .output()
        .expect("spawn elle");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `source` raises at top level and nothing catches it. The binary reports
/// the error whose message holds `message`, and exits 1.
///
/// Counter-factual: the raising callee inferred silent, and the call that
/// completed it aborted the process as a "silence violation" — a signal
/// death with no exit code, a "panic:" line, and no runtime error report.
fn reports_an_ordinary_error(source: &str, message: &str) {
    let (code, stderr) = run(source);
    assert!(
        !stderr.contains("panic"),
        "the run aborted for {source}; stderr:\n{stderr}"
    );
    assert_eq!(code, Some(1), "exit code for {source}; stderr:\n{stderr}");
    assert!(
        stderr.contains("Runtime error") && stderr.contains(message),
        "no runtime error naming {message:?} for {source}; stderr:\n{stderr}"
    );
}

#[test]
fn a_raise_from_a_function_that_once_inferred_silent_is_an_ordinary_error() {
    // `g` calls `f` in non-tail position, so `f`'s boundary is checked when
    // the call completes inside `g`.
    reports_an_ordinary_error(
        "(defn f [x] (def [a b] x) a) (defn g [] (def r (f 5)) r) (g)",
        "destructuring: expected array",
    );
}

#[test]
fn a_bound_violation_nothing_catches_is_an_ordinary_error() {
    reports_an_ordinary_error(
        "(defn aps [f x] (silence f) (f x)) (defn g [] (def r (aps + 42)) r) (g)",
        "restrict: closure may emit {:error} but parameter is restricted to {}",
    );
}

#[test]
fn a_raise_under_eval_is_an_eval_error() {
    reports_an_ordinary_error(
        "(eval '(begin (defn f [x] (def [a b] x) a) (defn g [] (def r (f 5)) r) (g)))",
        "destructuring: expected array",
    );
}
