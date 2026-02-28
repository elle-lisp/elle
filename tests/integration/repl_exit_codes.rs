// Tests for REPL exit codes with piped input
//
// Verifies that the REPL returns appropriate exit codes when parsing errors
// occur during piped input execution.

use std::io::Write;
use std::process::{Command, Stdio};

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn test_repl_piped_input_parse_error_exit_code() {
    // Test that piping parse errors to the REPL results in exit code 1
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle process at {}", elle_bin));

    // Write invalid input (unterminated list) to stdin
    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(+ 1 1\n")
            .expect("Failed to write to stdin");
    } // stdin is closed here

    let output = child.wait_with_output().expect("Failed to wait on child");

    // Should exit with status 1 due to parse error
    assert_eq!(
        output.status.code().unwrap_or(-1),
        1,
        "Expected exit code 1, stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify error was reported
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Parse error") || stderr.contains("unterminated"),
        "Expected parse error message in stderr, got: {}",
        stderr
    );
}

#[test]
fn test_repl_piped_input_successful_exit_code() {
    // Test that piping valid input to the REPL results in exit code 0
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle process at {}", elle_bin));

    // Write valid input to stdin
    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(+ 1 1)\n")
            .expect("Failed to write to stdin");
    } // stdin is closed here

    let output = child.wait_with_output().expect("Failed to wait on child");

    // Should exit with status 0 for successful execution
    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "Expected exit code 0, stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_repl_piped_input_multiple_errors_exit_code() {
    // Test that piping multiple invalid expressions results in exit code 1
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle process at {}", elle_bin));

    // Write multiple invalid inputs
    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(+ 1 1)\n")
            .expect("Failed to write to stdin");
        stdin
            .write_all(b"(* 2 2\n")
            .expect("Failed to write to stdin"); // This one has an error
    } // stdin is closed here

    let output = child.wait_with_output().expect("Failed to wait on child");

    // Should exit with status 1 due to parse error in the second expression
    assert_eq!(
        output.status.code().unwrap_or(-1),
        1,
        "Expected exit code 1, stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
