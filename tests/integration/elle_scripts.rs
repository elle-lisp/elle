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
    run_elle_script_with_args(name, &[]);
}

/// Like `run_elle_script` but passes extra args to the elle binary.
/// Used to gate scripts under non-default backends (e.g. `--no-uring`
/// for the threadpool I/O path, which is the only path on macOS and a
/// distinct codepath from io_uring on Linux).
fn run_elle_script_with_args(name: &str, extra_args: &[&str]) {
    let elle_bin = get_elle_binary();
    let script = format!("tests/elle/{}.lisp", name);

    let mut cmd = Command::new(elle_bin);
    cmd.args(extra_args).arg(&script);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn elle for {} {:?}: {}", script, extra_args, e));

    assert!(
        output.status.success(),
        "Elle script {} {:?} failed (exit {:?}):\nstdout: {}\nstderr: {}",
        script,
        extra_args,
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
    run_elle_script("fiber-io-stress");
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

#[test]
fn table_key_expand() {
    run_elle_script("table-key-expand");
}

#[test]
fn region_basic() {
    run_elle_script("region-basic");
}

#[test]
fn jit_string_push() {
    run_elle_script("jit-string-push");
}

#[test]
fn jit_bytes_push() {
    run_elle_script("jit-bytes-push");
}

#[test]
fn posix() {
    run_elle_script("posix");
}

/// Same script as `posix`, but forces the threadpool I/O backend on
/// Linux via `--no-uring`. The threadpool path uses the same
/// `SignalReceiver` / `kq_sig_read_blocking` / `sigfd_read_blocking`
/// machinery as macOS does, so this gates the threadpool signal flow
/// — the f7aed410 signalfd EAGAIN-poll fix on Linux and the EVFILT_SIGNAL
/// worker-unblock + no-op sigaction fix on macOS. Without this we'd
/// only exercise the io_uring path on the Linux runner.
#[test]
fn posix_threadpool() {
    run_elle_script_with_args("posix", &["--no-uring"]);
}
