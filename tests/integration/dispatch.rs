// CLI dispatch tests for lint and lsp subcommands

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn test_lint_help_exits_zero() {
    let output = Command::new(get_elle_binary())
        .args(["lint", "--help"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "elle lint --help should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_good_file_exits_zero() {
    let output = Command::new(get_elle_binary())
        .args(["lint", "tests/fixtures/naming-good.lisp"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "elle lint on clean file should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_naming_file_exits_zero() {
    // Kebab-case naming lint was removed — naming-bad.lisp now produces
    // zero diagnostics and exits 0.
    let output = Command::new(get_elle_binary())
        .args(["lint", "tests/fixtures/naming-bad.lisp"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "elle lint on naming-bad.lisp should exit 0 (no warnings), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_json_output() {
    let output = Command::new(get_elle_binary())
        .args(["lint", "--format", "json", "tests/fixtures/naming-bad.lisp"])
        .output()
        .expect("Failed to run elle");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"diagnostics\""),
        "JSON output should contain diagnostics key, got: {}",
        stdout
    );
}

#[test]
fn test_lint_nonexistent_file() {
    let output = Command::new(get_elle_binary())
        .args(["lint", "nonexistent-file-that-does-not-exist.lisp"])
        .output()
        .expect("Failed to run elle");

    // Should not exit 0 — the file doesn't exist
    assert_ne!(
        output.status.code().unwrap_or(-1),
        0,
        "elle lint on nonexistent file should not exit 0"
    );
}

#[test]
fn test_existing_file_execution_unchanged() {
    // Verify that normal file execution still works
    let output = Command::new(get_elle_binary())
        .args(["docs/syntax.md"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "Normal file execution should still work, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_help_mentions_lint_and_lsp() {
    let output = Command::new(get_elle_binary())
        .args(["--help"])
        .output()
        .expect("Failed to run elle");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lint"),
        "--help should mention lint, got: {}",
        stdout
    );
    assert!(
        stdout.contains("lsp"),
        "--help should mention lsp, got: {}",
        stdout
    );
}

#[test]
fn test_toplevel_unmet_gate_exits_zero_with_skip() {
    // A loud (gate! …) whose condition is unmet emits an uncaught :gated signal
    // at the top level. Run directly (not under the test runner), this must be a
    // clean SKIP — exit 0 with the reason on stderr — so gate! is a universal
    // skip mechanism. (Replaces the dangerous (sys/exit 0) idiom in service/FFI
    // tests.) Counter-factual: before the runtime handles :gated specially, an
    // uncaught :gated exited non-zero like any other error.
    let output = Command::new(get_elle_binary())
        .arg("tests/fixtures/gated-toplevel.lisp")
        .output()
        .expect("Failed to run elle");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "a top-level unmet gate! must exit 0 (skip), stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("SKIP") && stderr.contains("dependency absent"),
        "must report the gate reason as a skip on stderr, stderr: {}",
        stderr
    );
    assert!(
        !stdout.contains("must print"),
        "gated body and the following form must not run, stdout: {}",
        stdout
    );
}

#[test]
fn test_toplevel_ordinary_error_still_exits_nonzero() {
    // Guard: ONLY :gated is a clean skip. An ordinary uncaught error must still
    // fail, or we'd mask real failures as skips.
    let output = Command::new(get_elle_binary())
        .args(["-e", "(error \"boom\")"])
        .output()
        .expect("Failed to run elle");

    assert_ne!(
        output.status.code().unwrap_or(-1),
        0,
        "an ordinary uncaught error must still exit non-zero, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_rewrite_help() {
    let output = Command::new(get_elle_binary())
        .args(["rewrite", "--help"])
        .output()
        .expect("Failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("elle rewrite"));
}
