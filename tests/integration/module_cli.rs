// audited: 2026-11-20
// What a load of a file reports, and the location of a program with no file.
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

// The search has a subfile of its own, which opens with `use super::*;` so it
// sees the helpers above.
mod search {
    include!("module_cli/search.rs");
}
