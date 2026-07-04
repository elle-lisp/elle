// `os/spawn` workers must have a stack large enough to compile anything the
// main thread can. The `elle test` runner ships a file's syntax to a worker,
// which compiles it with its own stdlib; the frontend's HIR passes recurse
// depth-first, so a deep file overflows a worker spawned with Rust's default
// 2 MB stack (a raw SIGSEGV — exit 139 — with no panic message).
//
// Counter-factual: before workers were sized to the main thread's stack, this
// test crashed with exit 139. The fixture is calibrated so that compiling it
// in a 2 MB worker overflows, while the main thread's gating compile (which
// runs the same pass on the larger main stack) survives — so the only thing
// the test exercises is the worker's stack.

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Write a file with `n` trivial top-level forms plus a closing assert. A long
/// flat sequence of forms is what makes `functionalize` recurse deeply over the
/// whole-module body in the worker — without nesting parens (which would
/// instead overflow the *reader* on the main thread and abort before any worker
/// runs). 350 forms overflows a 2 MB worker but survives an 8 MB main thread.
fn write_deep_fixture(path: &std::path::Path, n: usize) {
    let mut f = std::fs::File::create(path).expect("create fixture");
    for i in 0..n {
        writeln!(f, "(def a{i} {i})").unwrap();
    }
    writeln!(f, "(assert (= a0 0))").unwrap();
}

#[test]
fn worker_stack_compiles_deep_file_without_overflow() {
    let dir = std::env::temp_dir().join("elle_spawn_stack_test");
    std::fs::create_dir_all(&dir).expect("mkdir tempdir");
    let fixture = dir.join("deep-worker-compile.lisp");
    let db = dir.join("deep-worker-compile.db");
    let _ = std::fs::remove_file(&db);
    write_deep_fixture(&fixture, 350);

    // Clear RUST_MIN_STACK so we exercise the *default* worker stack: before the
    // fix that default is Rust's 2 MB (→ SIGSEGV mid-compile); after the fix it
    // is the main thread's RLIMIT_STACK (ample). If a developer's shell exports
    // RUST_MIN_STACK, removing it here keeps the counter-factual honest.
    let output = Command::new(elle_binary())
        .args(["test"])
        .arg(&fixture)
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");

    // A stack overflow on a worker thread kills the whole process with a signal
    // (SIGSEGV = 11), so the child has no exit code — `code()` is None and the
    // signal is reported separately. That is the precise pre-fix symptom.
    assert_eq!(
        output.status.signal(),
        None,
        "elle test was killed by signal {:?} (SIGSEGV/11 = a worker overflowed \
         its stack mid-compile) — workers are not sized to the main thread's \
         stack. stderr:\n{}",
        output.status.signal(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "elle test on a deep but valid file should pass (exit 0); got {:?}. \
         stderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
