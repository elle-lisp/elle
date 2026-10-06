// audited: 2026-10-06
// Where a program finds a module, from a file or from none, and what a cycle of literal imports reports when it runs or is analyzed.
// docs/modules.md
// docs/config.md

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run the elle binary in `cwd` with `args`, feed it `stdin`, and hand back
/// (exit code, stdout, stderr). The environment search settings are cleared,
/// so the flags alone decide where a spec resolves.
fn run_elle(cwd: &Path, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(args)
        .current_dir(cwd)
        .env_remove("ELLE_PATH")
        .env_remove("ELLE_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait for elle");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A scratch directory holding `m.lisp`, a module whose value is 42.
fn module_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("m.lisp"), "42\n").expect("write module");
    dir
}

/// A bare spec never searches the working directory, so a file beside the
/// program cannot shadow a library of the same name. The counter-factual is
/// the search that put the working directory first: it loads `m.lisp`.
#[test]
fn a_bare_spec_does_not_search_the_working_directory() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import \"m\"))");
    assert_ne!(
        code, 0,
        "a bare spec must not find m.lisp in the working directory (stdout: {stdout})"
    );
    assert!(!stdout.contains("42"), "the module must not load: {stdout}");
    assert!(
        stderr.contains(":io-error"),
        "the failure is the resolution's :io-error: {stderr}"
    );
}

/// A program with no file has no directory of its own, so a relative
/// `import-file` path resolves against the working directory.
#[test]
fn import_file_on_stdin_resolves_against_the_working_directory() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import-file \"m.lisp\"))");
    assert_eq!(code, 0, "the load succeeds (stderr: {stderr})");
    assert_eq!(stdout.trim(), "42", "the module's value is printed");
}

/// `--path` names the directories a bare spec searches, and a relative entry
/// resolves against the working directory. The program runs from a sibling
/// directory, so only the `--path` entry can find the module.
#[test]
fn path_makes_a_bare_spec_resolve() {
    let root = tempfile::tempdir().expect("scratch dir");
    let lib = root.path().join("lib");
    let elsewhere = root.path().join("elsewhere");
    std::fs::create_dir_all(&lib).expect("create lib");
    std::fs::create_dir_all(&elsewhere).expect("create elsewhere");
    std::fs::write(lib.join("m.lisp"), "42\n").expect("write module");

    let flag = format!("--path={}", lib.display());
    let (code, stdout, stderr) = run_elle(&elsewhere, &[&flag, "-"], "(print (import \"m\"))");
    assert_eq!(
        code, 0,
        "an absolute --path entry finds m (stderr: {stderr})"
    );
    assert_eq!(stdout.trim(), "42");

    let (code, stdout, stderr) =
        run_elle(root.path(), &["--path=lib", "-"], "(print (import \"m\"))");
    assert_eq!(
        code, 0,
        "a relative --path entry finds m (stderr: {stderr})"
    );
    assert_eq!(stdout.trim(), "42");
}

/// Code on stdin and under `-e` has no file, so its location names none.
#[test]
fn a_program_with_no_file_has_a_nil_location_file() {
    let dir = module_dir();
    let (code, stdout, stderr) =
        run_elle(dir.path(), &["-"], "(print (get (meta/location) :file))");
    assert_eq!(code, 0, "stdin runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "nil", "stdin has no file");

    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["-e", "(print (get (meta/location) :file))"],
        "",
    );
    assert_eq!(code, 0, "-e runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "nil", "-e has no file");
}

/// `--trace=import` names each file a loader loads, as an absolute path.
#[test]
fn trace_import_names_the_loaded_file() {
    let dir = module_dir();
    let (code, _, stderr) = run_elle(
        dir.path(),
        &["--trace=import", "-"],
        "(import-file \"m.lisp\")",
    );
    assert_eq!(code, 0, "the load succeeds (stderr: {stderr})");
    let module = dir
        .path()
        .canonicalize()
        .expect("canonical scratch dir")
        .join("m.lisp");
    let traced = stderr
        .lines()
        .any(|l| l.starts_with("[trace:import]") && l.contains(&module.display().to_string()));
    assert!(
        traced,
        "a [trace:import] line names {}: {stderr}",
        module.display()
    );
}

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
/// starts at `b.lisp`. The counter-factual is the analysis that compiled each
/// literal import: it recursed on the cycle until the process aborted on a stack
/// overflow, and no exit code came back.
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
/// nothing a cycle can recurse through. The counter-factual aborts the process
/// on a stack overflow before `analyzed` is printed.
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
