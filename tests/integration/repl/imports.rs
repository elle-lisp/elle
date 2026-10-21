// audited: 2026-10-21
// What a file a REPL line imports cannot see: the macros and the definitions earlier lines made.
// docs/pipeline.md

use super::*;

/// A macro typed at the prompt reaches later lines, and not a file a line
/// imports: the file expands against the prelude and its own macros, as it
/// would from any program. The counter-factual merged REPL macros into the
/// instance's expander, so the import expanded `rmac` and answered 43.
#[test]
fn a_repl_macro_does_not_reach_an_imported_file() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("m.lisp"), "(+ (rmac) 1)\n").expect("write m.lisp");
    let (out, err, _) = elle_in(
        dir.path(),
        "(defmacro rmac [] 42)\n(rmac)\n(import-file \"m.lisp\")\n",
    );
    assert!(out.contains("⟹ 42"), "a later line expands rmac: {out}");
    assert!(
        !out.contains("⟹ 43"),
        "the import must not expand rmac: {out}"
    );
    assert!(
        err.contains("undefined variable: rmac"),
        "the import compiles without the REPL macro: {err}"
    );
}

/// A `def` typed at the prompt reaches later lines, and not a file a line
/// imports. The counter-factual registered REPL bindings with the instance,
/// so the import resolved `rdef` and answered 43.
#[test]
fn a_repl_def_does_not_reach_an_imported_file() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("m.lisp"), "(+ rdef 2)\n").expect("write m.lisp");
    let (out, err, _) = elle_in(
        dir.path(),
        "(def rdef 41)\n(+ rdef 1)\n(import-file \"m.lisp\")\n",
    );
    assert!(out.contains("⟹ 42"), "a later line resolves rdef: {out}");
    assert!(
        !out.contains("⟹ 43"),
        "the import must not resolve rdef: {out}"
    );
    assert!(
        err.contains("undefined variable: rdef"),
        "the import compiles without the REPL binding: {err}"
    );
}
