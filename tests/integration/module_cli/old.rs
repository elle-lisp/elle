// audited: 2026-11-20
// An old program and the old module of several forms it imports run unchanged, before and after elle rewrite.
//
// docs/epochs.md
// docs/modules.md
//
// The pair is the shape every program written before epoch 16 has: a module
// file of several forms beside a program that imports it by a bare spec, run
// from their directory.
use super::*;

const MODULE: &str = "(elle/epoch 12)\n\
                      (def greeting \"Hi\")\n\
                      (defn hi [n] (string greeting \" \" n))\n\
                      (fn [] {:hi hi})\n";
const PROGRAM: &str = "(elle/epoch 12)\n\
                       (def m ((import \"old\")))\n\
                       (print (m:hi \"x\"))\n";

/// A scratch directory holding the old module and the old program.
fn old_pair() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("old.lisp"), MODULE).expect("write old.lisp");
    std::fs::write(dir.path().join("main.lisp"), PROGRAM).expect("write main.lisp");
    dir
}

/// The counter-factual refused the module: `holds 3 forms, and a module holds
/// one`.
#[test]
fn an_old_program_imports_an_old_module_of_several_forms() {
    let dir = old_pair();
    let (code, stdout, stderr) = run_elle(dir.path(), &["main.lisp"], "");
    assert_eq!(code, 0, "the old program runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "Hi x");
}

/// `elle rewrite` folds the module into one form at the current epoch, and the
/// program still runs against it.
#[test]
fn the_rewritten_module_is_one_form_and_still_loads() {
    let dir = old_pair();
    let (code, _, stderr) = run_elle(dir.path(), &["rewrite", "old.lisp", "main.lisp"], "");
    assert_eq!(code, 0, "the rewrite succeeds (stderr: {stderr})");

    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["-"],
        "(print (length (compile/read-forms (file/read \"old.lisp\") \"old.lisp\")))",
    );
    assert_eq!(code, 0, "the module reads (stderr: {stderr})");
    assert_eq!(stdout.trim(), "1", "the rewritten module holds one form");

    let (code, stdout, stderr) = run_elle(dir.path(), &["main.lisp"], "");
    assert_eq!(code, 0, "the program runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "Hi x");
}
