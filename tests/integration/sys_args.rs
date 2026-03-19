// Integration tests for sys/args behavior.
//
// sys/args returns arguments that follow the source file (or stdin `-`) in
// the process argv. There is no `--` separator. These tests spawn the elle
// binary as a subprocess since the behavior is end-to-end (main.rs sets
// vm.user_args).

use std::io::Write;
use std::process::{Command, Stdio};

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn test_sys_args_no_trailing_args_returns_empty() {
    // Run `elle -` with stdin `(display (sys/args))` and no trailing args.
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
            .write_all(b"(display (sys/args))")
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
        "sys/args without trailing args should display as (), got: {:?}",
        stdout
    );
}

#[test]
fn test_sys_args_trailing_args_returned() {
    // Run `elle - foo bar` with stdin `(display (sys/args))`.
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
            .write_all(b"(display (sys/args))")
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
    // Run `elle - -v foo` with stdin `(display (sys/args))`.
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
            .write_all(b"(display (sys/args))")
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
