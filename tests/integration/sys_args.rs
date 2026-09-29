// audited: 2026-09-29
// What reaches the running program: `sys/args` and `sys/argv`, end to end.
//
// docs/config.md
//
// Every argument after the source file (or stdin `-`) belongs to the program, a
// `--` included, and no separator is needed to say so. These tests spawn the
// binary, because `main` is what fills `vm.user_args`.

use std::io::Write;
use std::process::{Command, Stdio};

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `source` on stdin under `args` and answer `(stdout, stderr)`.
fn run_stdin(args: &[&str], source: &str) -> (String, String) {
    let mut child = Command::new(get_elle_binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait on elle");
    assert!(
        out.status.success(),
        "elle {:?} exited {:?}\nstderr: {}",
        args,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    )
}

// ── `--` hands the rest to the program, whatever it spells ──

/// A flag named like one of elle's own still belongs to the program once `--`
/// has ended elle's flags.
///
/// The counter-factual: a `main` that scans the whole argv for `--help` and
/// `--version` answers these two with the banner, and the program never runs,
/// while every other flag after `--` reaches the program (`--trace=call` below).
/// A script could then carry no flag of either name.
#[test]
fn help_after_the_separator_belongs_to_the_program() {
    let (out, _) = run_stdin(&["-", "--", "--help"], "(print (sys/args))");
    assert_eq!(out, "(-- --help)");
}

#[test]
fn version_after_the_separator_belongs_to_the_program() {
    let (out, _) = run_stdin(&["-", "--", "--version"], "(print (sys/args))");
    assert_eq!(out, "(-- --version)");
}

/// The other half of the pair: before the source, both are still elle's.
#[test]
fn help_and_version_before_the_separator_are_elles_own() {
    let out = Command::new(get_elle_binary())
        .arg("--version")
        .output()
        .expect("spawn elle");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        elle::BANNER,
        "--version before any source argument is elle's"
    );

    let out = Command::new(get_elle_binary())
        .arg("--help")
        .output()
        .expect("spawn elle");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Usage: elle"),
        "--help before any source argument is elle's"
    );
}

/// `--` ends elle's flags for every flag, which is the rule `--help` and
/// `--version` have to join rather than a special case written for them.
#[test]
fn an_ordinary_flag_after_the_separator_reaches_the_program_untouched() {
    let (out, _) = run_stdin(
        &["-", "--", "--trace=call"],
        "(print (sys/args)) (println \" trace=\" (vm/config :trace))",
    );
    assert_eq!(
        out, "(-- --trace=call) trace=||",
        "the program gets the flag and tracing stays off"
    );
}

// ── `sys/args` and `sys/argv` ──

/// With nothing after the source, the program has no arguments: an empty list,
/// never nil.
#[test]
fn sys_args_is_empty_with_no_trailing_arguments() {
    let (out, _) = run_stdin(&["-"], "(print (sys/args))");
    assert_eq!(out, "()");
}

#[test]
fn sys_args_holds_every_trailing_argument() {
    let (out, _) = run_stdin(&["-", "foo", "bar"], "(print (sys/args))");
    assert_eq!(out, "(foo bar)");
}

/// A flag after the source is the program's, not elle's.
#[test]
fn a_flag_after_the_source_is_a_program_argument() {
    let (out, _) = run_stdin(&["-", "-v", "foo"], "(print (sys/args))");
    assert_eq!(out, "(-v foo)");
}

/// `sys/argv` is `sys/args` with the source name in front: `-` for stdin.
#[test]
fn sys_argv_puts_the_source_name_first() {
    let (out, _) = run_stdin(&["-", "foo", "bar"], "(print (sys/argv))");
    assert_eq!(out, "(- foo bar)");
    let (out, _) = run_stdin(&["-"], "(print (sys/argv))");
    assert_eq!(out, "(-)", "with no trailing arguments, only the source name");
    let (out, _) = run_stdin(&["-", "-v", "foo"], "(print (sys/argv))");
    assert_eq!(out, "(- -v foo)");
}
