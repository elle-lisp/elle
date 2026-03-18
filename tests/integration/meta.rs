// Integration tests for meta/origin primitive.
//
// meta/origin returns the source location of a closure as {:file :line :col},
// or nil if unavailable. These tests spawn the elle binary with a temp file
// to verify that source location tracking flows through the full pipeline.

use std::io::Write;
use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn meta_origin_returns_file_and_line_for_defn() {
    // Write a small script that defines a function and prints meta/origin on it.
    let path = std::env::temp_dir().join("elle_meta_origin_test.lisp");
    std::fs::write(&path, "(defn foo () 42)\n(print (meta/origin foo))\n")
        .expect("failed to write temp file");

    let path_str = path.to_str().expect("path not UTF-8").to_string();

    let output = Command::new(get_elle_binary())
        .arg(&path_str)
        .output()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", get_elle_binary()));

    // Clean up regardless of outcome.
    let _ = std::fs::remove_file(&path);

    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "elle exited with error\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );

    // The output should contain the file path and a line number.
    // meta/origin returns {:file "..." :line N :col N}.
    assert!(
        stdout.contains(&path_str),
        "expected file path {:?} in output, got: {:?}",
        path_str,
        stdout
    );
    assert!(
        stdout.contains(":line"),
        "expected :line key in output, got: {:?}",
        stdout
    );
    assert!(
        stdout.contains(":col"),
        "expected :col key in output, got: {:?}",
        stdout
    );
}

#[test]
fn meta_origin_non_closure_returns_nil() {
    let elle_bin = get_elle_binary();

    let mut child = Command::new(elle_bin)
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Failed to spawn elle at {}", elle_bin));

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"(print (meta/origin 42))")
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
        "nil",
        "meta/origin on non-closure should print nil, got: {:?}",
        stdout
    );
}
