// audited: 2026-09-14
// What reaches the running program: `sys/args` and `sys/argv`, end to end.
//
// Every argument after the source file (or stdin `-`) belongs to the program,
// and no separator is needed to say so. `--` is not consumed either — elle
// stops reading its own flags there and passes the rest through verbatim.
// These tests spawn the binary, because `main` is what fills `vm.user_args`.
//
// docs/config.md

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
/// The counter-factual: scan the whole argv for `--help` and `--version`, the
/// way `main` did, and these two answer with the banner while the program
/// never runs. Every other flag already passes through — `--jit=off` after
/// `--` reaches the program and leaves the tier alone — so the two were the
/// sole exception, and a script could not carry a flag of either name.
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

/// The other half of the pair: before the separator both are still elle's, so
/// the fix must move the boundary rather than drop the flags.
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
        &["-", "--", "--jit=off"],
        "(print (sys/args)) (println \" jit=\" (vm/config :jit))",
    );
    assert_eq!(
        out, "(-- --jit=off) jit=adaptive",
        "the program gets the flag and the tier keeps its default"
    );
}

#[test]
fn test_sys_args_no_trailing_args_returns_empty() {
    // Run `elle -` with stdin `(print (sys/args))` and no trailing args.
    // sys/args should return () — display of empty list is "()".
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/args))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout.trim(),
        "()",
        "sys/args without trailing args should print as (), got: {:?}",
        stdout
    );
}

#[test]
fn test_sys_args_trailing_args_returned() {
    // Run `elle - foo bar` with stdin `(print (sys/args))`.
    // sys/args should return ("foo" "bar").
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .args(["-", "foo", "bar"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/args))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("foo"),
        "expected 'foo' in sys/args output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains("bar"),
        "expected 'bar' in sys/args output, got: {:?}",
        stdout
    );
}

#[test]
fn test_sys_args_flags_after_source_passed_through() {
    // Run `elle - -v foo` with stdin `(print (sys/args))`.
    // Flags that appear after the source arg are passed through as user args,
    // not interpreted by elle.
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .args(["-", "-v", "foo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/args))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("-v"),
        "expected '-v' in sys/args output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains("foo"),
        "expected 'foo' in sys/args output, got: {:?}",
        stdout
    );
}

// --- sys/argv ---

#[test]
fn test_sys_argv_includes_script_name() {
    // Run `elle - foo bar` with stdin `(print (sys/argv))`.
    // sys/argv should include "-" as element 0, then "foo", "bar".
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .args(["-", "foo", "bar"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/argv))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // print of a list outputs elements without quotes: (- foo bar)
    // so "-" appears as a bare hyphen in the output.
    assert!(
        stdout.contains('-'),
        "expected '-' in sys/argv output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains("foo"),
        "expected 'foo' in sys/argv output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains("bar"),
        "expected 'bar' in sys/argv output, got: {:?}",
        stdout
    );
}

#[test]
fn test_sys_argv_no_trailing_args() {
    // Run `elle -` with stdin `(print (sys/argv))` and no trailing args.
    // sys/argv should return ("-") — a one-element list containing just the script name.
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/argv))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Should contain "-" as the only element; output should be a single-element list.
    assert!(
        stdout.contains('-'),
        "expected '-' in sys/argv output, got: {:?}",
        stdout
    );
    assert!(
        !stdout.contains("foo") && !stdout.contains("bar"),
        "sys/argv with no trailing args should not contain user args, got: {:?}",
        stdout
    );
}

#[test]
fn test_sys_argv_flags_after_source() {
    // Run `elle - -v foo` with stdin `(print (sys/argv))`.
    // Flags that appear after the source arg are passed through as user args.
    // sys/argv should include "-", "-v", "foo".
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .args(["-", "-v", "foo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (sys/argv))")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");

    assert!(
        output.status.success(),
        "elle exited with error, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("-v"),
        "expected '-v' in sys/argv output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains("foo"),
        "expected 'foo' in sys/argv output, got: {:?}",
        stdout
    );
}
