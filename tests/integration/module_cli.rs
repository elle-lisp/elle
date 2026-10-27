// audited: 2026-10-26
// Where a program finds a module, and what a load of a file reports.
//
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
/// program cannot shadow a library of the same name. The counter-factual
/// would search the working directory first, and would load `m.lisp`.
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

/// `import/load-file` takes a path as `file/read` does, so a relative path
/// resolves against the working directory.
#[test]
fn load_file_on_stdin_resolves_against_the_working_directory() {
    let dir = module_dir();
    let (code, stdout, stderr) =
        run_elle(dir.path(), &["-"], "(print (import/load-file \"m.lisp\"))");
    assert_eq!(code, 0, "the load succeeds (stderr: {stderr})");
    assert_eq!(stdout.trim(), "42", "the module's value is printed");
}

/// A program with no file has no directory of its own, so a `./` spec names
/// nothing. The counter-factual resolved it against the working directory and
/// loaded `m.lisp`.
#[test]
fn a_relative_spec_on_stdin_names_nothing() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], "(print (import \"./m\"))");
    assert_ne!(code, 0, "the import must fail (stdout: {stdout})");
    assert!(!stdout.contains("42"), "the module must not load: {stdout}");
    assert!(
        stderr.contains(":io-error"),
        "the failure is the resolution's :io-error: {stderr}"
    );
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

/// A load that fails keeps nothing, so the next load of the file fails again.
/// The counter-factual kept the failure beside the code, and the second load
/// answered without reading the file.
#[test]
fn a_failed_load_fails_again_on_the_next_load() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("bad.lisp"), "(no-such-name)\n").expect("write bad.lisp");
    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["-"],
        "(def [a? _] (protect (import/load-file \"bad.lisp\")))\n\
         (def [b? _] (protect (import/load-file \"bad.lisp\")))\n\
         (print [a? b?])",
    );
    assert_eq!(code, 0, "the program runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "[false false]", "both loads fail");
}

/// `compile/analyze` runs no code, and reads no file a literal load names, so
/// two programs whose literal loads name each other analyze. The
/// counter-factual compiled each literal load during analysis, and recursed
/// until the process aborted on a stack overflow before `analyzed` printed.
#[test]
fn analyzing_a_cycle_of_literal_loads_ends() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let a = dir.path().join("a.lisp").display().to_string();
    let b = dir.path().join("b.lisp").display().to_string();
    std::fs::write(
        &a,
        format!("(def b ((import/load-file \"{b}\")))\n(fn [] {{:b b}})\n"),
    )
    .expect("write a.lisp");
    std::fs::write(
        &b,
        format!("(def a ((import/load-file \"{a}\")))\n(fn [] {{:a a}})\n"),
    )
    .expect("write b.lisp");
    let program =
        format!("(compile/analyze (file/read \"{a}\") {{:file \"{a}\"}})\n(print \"analyzed\")\n");
    let (code, stdout, stderr) = run_elle(dir.path(), &["-"], &program);
    assert_eq!(code, 0, "the analysis returns (stderr: {stderr})");
    assert_eq!(stdout.trim(), "analyzed");
}
