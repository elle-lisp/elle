// CLI dispatch tests for --lint and --lsp switches

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn test_lint_help_exits_zero() {
    let output = Command::new(get_elle_binary())
        .args(["--lint", "--help"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "elle --lint --help should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_good_file_exits_zero() {
    let output = Command::new(get_elle_binary())
        .args(["--lint", "tests/fixtures/naming-good.lisp"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "elle --lint on clean file should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_bad_file_exits_two() {
    let output = Command::new(get_elle_binary())
        .args(["--lint", "tests/fixtures/naming-bad.lisp"])
        .output()
        .expect("Failed to run elle");

    assert_eq!(
        output.status.code().unwrap_or(-1),
        2,
        "elle --lint on bad-naming file should exit 2 (warnings), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_lint_json_output() {
    let output = Command::new(get_elle_binary())
        .args([
            "--lint",
            "--format",
            "json",
            "tests/fixtures/naming-bad.lisp",
        ])
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
        .args(["--lint", "nonexistent-file-that-does-not-exist.lisp"])
        .output()
        .expect("Failed to run elle");

    // Should not exit 0 — the file doesn't exist
    assert_ne!(
        output.status.code().unwrap_or(-1),
        0,
        "elle --lint on nonexistent file should not exit 0"
    );
}

#[test]
fn test_existing_file_execution_unchanged() {
    // Verify that normal file execution still works
    let output = Command::new(get_elle_binary())
        .args(["examples/basics.lisp"])
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
        stdout.contains("--lint"),
        "--help should mention --lint, got: {}",
        stdout
    );
    assert!(
        stdout.contains("--lsp"),
        "--help should mention --lsp, got: {}",
        stdout
    );
}
