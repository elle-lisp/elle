// audited: 2026-11-20
// Where import/resolve looks for a spec: the working directory, the path, home, and the writer's directory.
//
// docs/modules/resolution.md
// docs/config.md
//
// Each test writes its modules into a scratch directory and runs elle from a
// directory it picks, so the working directory is part of every case.
use super::*;

/// Write `value` as the module at `rel` under `root`, creating the directories
/// that `rel` names.
fn write_module(root: &Path, rel: &str, value: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("a module has a directory"))
        .expect("create the module's directory");
    std::fs::write(path, format!("{value}\n")).expect("write the module");
}

/// A bare spec searches the working directory. The counter-factual searched
/// the path and home alone, and failed with an `:io-error`.
#[test]
fn a_bare_spec_searches_the_working_directory() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import \"m\"))");
    assert_eq!(
        code, 0,
        "m.lisp in the working directory loads (stderr: {stderr})"
    );
    assert_eq!(stdout.trim(), "42");
}

/// A bare spec that names a directory, as `"lib/redis.lisp"` does, resolves
/// under the working directory too.
#[test]
fn a_bare_spec_with_a_directory_searches_the_working_directory() {
    let dir = tempfile::tempdir().expect("scratch dir");
    write_module(dir.path(), "sub/m.lisp", "7");
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import \"sub/m.lisp\"))");
    assert_eq!(code, 0, "sub/m.lisp loads (stderr: {stderr})");
    assert_eq!(stdout.trim(), "7");
}

/// The working directory comes before each `--path` entry. The counter-factual
/// searched the path first and printed 2.
#[test]
fn the_working_directory_comes_before_the_path() {
    let root = tempfile::tempdir().expect("scratch dir");
    write_module(root.path(), "m.lisp", "1");
    write_module(root.path(), "lib/m.lisp", "2");
    let (code, stdout, stderr) =
        run_elle(root.path(), &["--path=lib", "-"], "(print (import \"m\"))");
    assert_eq!(code, 0, "the import succeeds (stderr: {stderr})");
    assert_eq!(stdout.trim(), "1", "the working directory's m wins");
}

/// A `std/` spec resolves before the search, so a file of the same name in the
/// working directory does not shadow the standard library.
#[test]
fn a_std_spec_is_not_shadowed_by_the_working_directory() {
    let dir = tempfile::tempdir().expect("scratch dir");
    write_module(dir.path(), "std/base64.lisp", "42");
    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["-"],
        "(let [b ((import \"std/base64\"))] (print (b:encode \"hi\")))",
    );
    assert_eq!(code, 0, "std/base64 loads (stderr: {stderr})");
    assert_eq!(stdout.trim(), "aGk=", "the library's base64 answered");
}

/// `--path` names the directories a bare spec searches, and a relative entry
/// resolves against the working directory. The program runs from a sibling
/// directory, so only the `--path` entry can find the module.
#[test]
fn path_makes_a_bare_spec_resolve() {
    let root = tempfile::tempdir().expect("scratch dir");
    let elsewhere = root.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("create elsewhere");
    write_module(root.path(), "lib/m.lisp", "42");

    let flag = format!("--path={}", root.path().join("lib").display());
    let (code, stdout, stderr) = run_elle(&elsewhere, &[&flag, "-"], "(print (import \"m\"))");
    assert_eq!(
        code, 0,
        "an absolute --path entry finds m (stderr: {stderr})"
    );
    assert_eq!(stdout.trim(), "42");

    let (code, stdout, stderr) = run_elle(
        &elsewhere,
        &["--path=../lib", "-"],
        "(print (import \"m\"))",
    );
    assert_eq!(
        code, 0,
        "a relative --path entry finds m (stderr: {stderr})"
    );
    assert_eq!(stdout.trim(), "42");
}

/// A program with no file has no directory of its own, so a `./` spec names a
/// file in the working directory. The counter-factual named nothing.
#[test]
fn a_relative_spec_on_stdin_resolves_against_the_working_directory() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import \"./m\"))");
    assert_eq!(code, 0, "./m loads (stderr: {stderr})");
    assert_eq!(stdout.trim(), "42");
}

/// A `./` spec that names nothing beside its writer is looked for in the
/// working directory. The program lives in `prog/` and runs from the root,
/// where `m.lisp` is. The counter-factual looked in `prog/` alone.
#[test]
fn a_relative_spec_falls_back_to_the_working_directory() {
    let root = tempfile::tempdir().expect("scratch dir");
    write_module(root.path(), "m.lisp", "42");
    write_module(root.path(), "prog/main.lisp", "(print (import \"./m\"))");
    let (code, stdout, stderr) = run_elle(root.path(), &["prog/main.lisp"], "");
    assert_eq!(code, 0, "./m falls back to the root (stderr: {stderr})");
    assert_eq!(stdout.trim(), "42");
}

/// A `./` spec tries its writer's directory before the working directory.
#[test]
fn a_relative_spec_prefers_the_writers_directory() {
    let root = tempfile::tempdir().expect("scratch dir");
    write_module(root.path(), "m.lisp", "2");
    write_module(root.path(), "prog/m.lisp", "1");
    write_module(root.path(), "prog/main.lisp", "(print (import \"./m\"))");
    let (code, stdout, stderr) = run_elle(root.path(), &["prog/main.lisp"], "");
    assert_eq!(code, 0, "the import succeeds (stderr: {stderr})");
    assert_eq!(stdout.trim(), "1", "the writer's m wins");
}
