// audited: 2026-09-21
// The binary's positional contract: one program file, then that program's argv.
// docs/config.md

use std::process::Command;

/// Run the elle binary with `args` and hand back (exit code, stdout, stderr).
fn run_elle(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(args)
        .output()
        .expect("spawn elle");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The usage line matches what the binary does: it runs ONE file, and the
/// arguments after it are the program's.
///
/// The counter-factual: the line read `elle [file...] [-- args...]`, and a
/// caller who believed it — a Makefile passing a glob — got exit 0 with every
/// file after the first unrun. The files were not dropped (they reach the
/// program through `sys/args`), but nothing advertised that reading.
#[test]
fn the_usage_line_advertises_one_file() {
    let (code, stdout, _) = run_elle(&["--help"]);
    assert_eq!(code, 0, "--help exits 0");
    assert!(
        stdout.contains("elle [file] [args...]"),
        "the usage line names one file and the program's args: {stdout}"
    );
    assert!(
        !stdout.contains("[file...]"),
        "the usage line must not advertise multiple program files: {stdout}"
    );
}

/// Everything after the first file is the program's argv — a second `.lisp`
/// path included. It is an argument, not a second program: it does not run,
/// and the program reads it from `sys/args`.
#[test]
fn everything_after_the_file_is_the_programs_argv() {
    let dir = std::env::temp_dir().join(format!("elle-argv-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let main = dir.join("main.lisp");
    let second = dir.join("second.lisp");
    std::fs::write(&main, "(print (sys/args))").expect("write main");
    std::fs::write(&second, "(print \"second ran\")").expect("write second");

    let second_path = second.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run_elle(&[main.to_str().expect("utf-8 path"), second_path, "8080"]);
    std::fs::remove_dir_all(&dir).expect("remove scratch dir");

    assert_eq!(code, 0, "the run exits 0 (stderr: {stderr})");
    assert!(
        !stdout.contains("second ran"),
        "the second file is an argument, so it must not run: {stdout}"
    );
    assert!(
        stdout.contains(second_path) && stdout.contains("8080"),
        "the program reads both arguments from sys/args: {stdout}"
    );
}
