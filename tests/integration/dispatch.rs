// audited: 2026-10-26
// The elle binary dispatches its subcommands and its top level: lint, rewrite, --help, and a gated file.

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
    // The linter checks no naming convention, so naming-bad.lisp produces
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

/// A program whose one diagnostic is an error at 2:2: `g` is never bound.
const UNDEFINED: &str = "(elle/epoch 15)\n(g)\n";

fn lint(args: &[&str]) -> (i32, String) {
    let output = Command::new(get_elle_binary())
        .arg("lint")
        .args(args)
        .output()
        .expect("Failed to run elle");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn scratch_file(dir: &crate::common::ScratchDir, name: &str, content: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("write scratch file");
    path.to_str().expect("UTF-8 scratch path").to_string()
}

#[test]
fn lint_help_names_info_as_the_default_level() {
    // Counter-factual: the help said `warning (default)` while the linter
    // started from `info`.
    let (code, stdout) = lint(&["--help"]);
    assert_eq!(code, 0);
    let level = stdout
        .lines()
        .find(|l| l.contains("--level"))
        .unwrap_or_else(|| panic!("no --level line in: {stdout}"));
    assert!(level.contains("info (default)"), "got: {level}");
    assert_eq!(level.matches("(default)").count(), 1, "got: {level}");
}

#[test]
fn lint_text_names_the_file_of_each_diagnostic() {
    // Counter-factual: a line read `2:2 error: …`, and over several files the
    // reader could not tell which file it was about.
    let dir = crate::common::ScratchDir::new("lint-text-file");
    let a = scratch_file(&dir, "a.lisp", UNDEFINED);
    let (code, stdout) = lint(&[&a]);
    assert_eq!(code, 1, "stdout: {stdout}");
    assert!(
        stdout.contains(&format!("{a}:2:2 error: undefined-variable")),
        "got: {stdout}"
    );
}

#[test]
fn lint_a_directory_prints_its_files_in_sorted_order() {
    // Eight names, written in reverse: a directory listing that happened to
    // come back sorted would pass for the wrong reason.
    let dir = crate::common::ScratchDir::new("lint-dir-order");
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let paths: Vec<String> = names
        .iter()
        .rev()
        .map(|n| scratch_file(&dir, &format!("{n}.lisp"), UNDEFINED))
        .rev()
        .collect();
    let root = dir.path().to_str().expect("UTF-8 scratch path");
    let (code, stdout) = lint(&[root]);
    assert_eq!(code, 1, "stdout: {stdout}");
    let at: Vec<usize> = paths
        .iter()
        .map(|p| {
            stdout
                .find(&format!("{p}:2:2"))
                .unwrap_or_else(|| panic!("{p} missing from: {stdout}"))
        })
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "got: {stdout}");
}

#[test]
fn lint_json_names_the_file_of_each_diagnostic() {
    let dir = crate::common::ScratchDir::new("lint-json-file");
    let a = scratch_file(&dir, "a.lisp", UNDEFINED);
    let (code, stdout) = lint(&["--format", "json", &a]);
    assert_eq!(code, 1, "stdout: {stdout}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("JSON output");
    let diagnostics = json["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "got: {stdout}");
    assert_eq!(diagnostics[0]["file"], a.as_str(), "got: {stdout}");
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
    // skip mechanism, which service and FFI tests use in place of (sys/exit 0).
    // Counter-factual: before the runtime handles :gated specially, an
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

#[test]
fn rewrite_list_rules_names_the_calls_whose_durations_moved_to_seconds() {
    // Epoch 14 rewrites a millisecond bound as `:timeout` in seconds. A rule
    // the listing leaves out is a rewrite nobody reading it knows about.
    let output = Command::new(get_elle_binary())
        .args(["rewrite", "--list-rules"])
        .output()
        .expect("Failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for call in [
        "sys/join",
        "chan/select",
        "io/wait",
        "ev/step",
        "udp/recv-from",
    ] {
        assert!(
            stdout.contains(call),
            "--list-rules must name {call}, got: {stdout}"
        );
    }
}
