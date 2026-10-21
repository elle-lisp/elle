// audited: 2026-10-21
// What a cycle of literal imports reports, run as a program and analyzed.
//
// docs/modules.md

use super::*;

/// A scratch directory holding `a.lisp` and `b.lisp`, which import each other
/// through literal `import-file` paths at their top levels.
fn cycle_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(
        dir.path().join("a.lisp"),
        "(def b ((import-file \"b.lisp\")))\n(fn [] {:b b})\n",
    )
    .expect("write a.lisp");
    std::fs::write(
        dir.path().join("b.lisp"),
        "(def a ((import-file \"a.lisp\")))\n(fn [] {:a a})\n",
    )
    .expect("write b.lisp");
    dir
}

/// A cycle of literal imports reaches the loader, which names every file in it.
/// The program file runs without a load mark, so the cycle the loader sees
/// starts at `b.lisp`. The counter-factual would compile each literal import
/// during analysis: it would recurse on the cycle until the process aborted on
/// a stack overflow, with no exit code.
#[test]
fn a_cycle_of_literal_imports_names_the_cycle() {
    let dir = cycle_dir();
    let a = dir.path().join("a.lisp").display().to_string();
    let b = dir.path().join("b.lisp").display().to_string();
    let (code, _, stderr) = run_elle(dir.path(), &[&a], "");
    assert_eq!(
        code, 1,
        "the run fails with an error, not an abort: {stderr}"
    );
    let cycle = format!("circular dependency: {b} -> {a} -> {b}");
    assert!(
        stderr.contains(&cycle),
        "the error names the cycle `{cycle}`: {stderr}"
    );
}

/// A module that imports itself is a cycle of one file.
#[test]
fn a_literal_self_import_names_the_cycle() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let s = dir.path().join("s.lisp");
    std::fs::write(&s, "(def s ((import-file \"s.lisp\")))\n(fn [] {:z 3})\n")
        .expect("write s.lisp");
    let s = s.display().to_string();
    let (code, _, stderr) = run_elle(dir.path(), &[&s], "");
    assert_eq!(
        code, 1,
        "the run fails with an error, not an abort: {stderr}"
    );
    let cycle = format!("circular dependency: {s} -> {s}");
    assert!(
        stderr.contains(&cycle),
        "the error names the cycle `{cycle}`: {stderr}"
    );
}

/// `compile/analyze` runs no code, and with no compile of an import there is
/// nothing a cycle can recurse through. The counter-factual would abort the
/// process on a stack overflow before `analyzed` is printed.
#[test]
fn analyzing_a_cycle_of_literal_imports_ends() {
    let dir = cycle_dir();
    let a = dir.path().join("a.lisp").display().to_string();
    let program =
        format!("(compile/analyze (slurp \"{a}\") {{:file \"{a}\"}})\n(print \"analyzed\")\n");
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], &program);
    assert_eq!(code, 0, "the analysis returns (stderr: {stderr})");
    assert_eq!(stdout.trim(), "analyzed");
}
