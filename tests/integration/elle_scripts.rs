// Elle script integration tests.
//
// Runs Elle scripts in tests/elle/ as subprocess tests via the elle binary.
// Each script exits 0 on success, 1 on assertion failure.
//
// To add a new script test:
//   1. Create tests/elle/myfeature.lisp
//   2. Add: #[test] fn myfeature() { run_elle_script("myfeature"); }

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run tests/elle/{name}.lisp and assert it exits with code 0.
///
/// Panics with stdout+stderr output if the script exits non-zero or fails to spawn.
fn run_elle_script(name: &str) {
    let elle_bin = get_elle_binary();
    let script = format!("tests/elle/{}.lisp", name);

    let output = Command::new(elle_bin)
        .arg(&script)
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn elle for {}: {}", script, e));

    assert!(
        output.status.success(),
        "Elle script {} failed (exit {:?}):\nstdout: {}\nstderr: {}",
        script,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// =============================================================================
// JIT regression tests
// =============================================================================

#[test]
fn jit() {
    run_elle_script("jit");
}

#[test]
fn file_stat() {
    run_elle_script("file-stat");
}

#[test]
fn errors() {
    run_elle_script("errors");
}

#[test]
fn fiber_stress() {
    run_elle_script("fiber-stress");
}

#[test]
#[ignore] // JIT leaks raw io-request structs after repeated sequential reads
fn fiber_io_stress() {
    run_elle_script("fiber_io_stress");
}

#[test]
fn caps() {
    run_elle_script("caps");
}

#[test]
fn emit() {
    run_elle_script("emit");
}

#[test]
fn grpc() {
    run_elle_script("grpc");
}

#[test]
fn websocket() {
    run_elle_script("websocket");
}
