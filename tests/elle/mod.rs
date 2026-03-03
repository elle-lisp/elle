// Elle script tests — each .lisp file in this directory is a test.
//
// Scripts use assertions.lisp helpers (assert-eq, assert-true, etc.)
// which call (exit 1) on failure. A zero exit code means all
// assertions passed.

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run_elle_script(name: &str) {
    let path = format!("tests/elle/{}.lisp", name);
    let output = Command::new(get_elle_binary())
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run elle on {}: {}", path, e));

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "{} failed (exit code {:?}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
            path,
            output.status.code(),
            stdout,
            stderr
        );
    }
}

#[test]
fn eval() {
    run_elle_script("eval");
}

#[test]
fn prelude() {
    run_elle_script("prelude");
}
