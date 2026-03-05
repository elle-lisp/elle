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

#[test]
fn destructuring() {
    run_elle_script("destructuring");
}

#[test]
fn core() {
    run_elle_script("core");
}

#[test]
fn splice() {
    run_elle_script("splice");
}

#[test]
fn blocks() {
    run_elle_script("blocks");
}

#[test]
fn functional() {
    run_elle_script("functional");
}

#[test]
fn arithmetic() {
    run_elle_script("arithmetic");
}

#[test]
fn determinism() {
    run_elle_script("determinism");
}

#[test]
fn property_eval() {
    run_elle_script("property-eval");
}

#[test]
fn convert() {
    run_elle_script("convert");
}

#[test]
fn sequences() {
    run_elle_script("sequences");
}

#[test]
fn macros() {
    run_elle_script("macros");
}

#[test]
fn strings() {
    run_elle_script("strings");
}

#[test]
fn bugfixes() {
    run_elle_script("bugfixes");
}

#[test]
fn fibers() {
    run_elle_script("fibers");
}

#[test]
fn closures() {
    run_elle_script("closures");
}
