// audited: 2026-10-21
// Where a program finds a module, and how often a file that loads twice compiles.
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

/// How many compiles of `file` a `--trace=compile` run reported: each compile
/// traces its read phase once, under the file's path.
fn compiles_of(stderr: &str, file: &str) -> usize {
    let read = format!("{file} read ");
    stderr
        .lines()
        .filter(|l| l.starts_with("[trace:compile]") && l.contains(&read))
        .count()
}

/// Two loads of one file compile it once, and both run it. The
/// counter-factual would compile the file on every load, and its read phase
/// would appear twice.
#[test]
fn two_loads_of_one_file_compile_it_once() {
    let dir = module_dir();
    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["--trace=compile", "-"],
        "(print (+ (import-file \"m.lisp\") (import-file \"m.lisp\")))",
    );
    assert_eq!(code, 0, "both loads succeed (stderr: {stderr})");
    assert_eq!(stdout.trim(), "84", "both loads ran the file");
    assert_eq!(
        compiles_of(&stderr, "m.lisp"),
        1,
        "m.lisp compiled once: {stderr}"
    );
}

/// A compile that fails keeps nothing, so the next load of the file compiles
/// it again and reports the error again. The counter-factual would keep the
/// failure beside the code, and the second load would compile nothing.
#[test]
fn a_failed_compile_compiles_again_on_the_next_load() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("bad.lisp"), "(no-such-name)\n").expect("write bad.lisp");
    let (code, stdout, stderr) = run_elle(
        dir.path(),
        &["--trace=compile", "-"],
        "(def [a? _] (protect (import-file \"bad.lisp\")))\n\
         (def [b? _] (protect (import-file \"bad.lisp\")))\n\
         (print [a? b?])",
    );
    assert_eq!(code, 0, "the program runs (stderr: {stderr})");
    assert_eq!(stdout.trim(), "[false false]", "both loads fail");
    assert_eq!(
        compiles_of(&stderr, "bad.lisp"),
        2,
        "bad.lisp compiled on each load: {stderr}"
    );
}

mod cycle {
    include!("module_cli/cycle.rs");
}
