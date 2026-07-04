// The runner must trap a test that calls `exit` so it cannot silently truncate
// the whole run.
//
// `exit` lowers to `std::process::exit` — uncatchable, process-wide. A test file
// runs as a whole-file thunk in a worker; if it calls `(exit 0)` the entire
// `elle test` process ends then and there, and every file scheduled after it is
// silently dropped (the run "passes" with a partial result set). The runner sets
// a per-thread exit trap around each test's execution so `exit` instead emits a
// catchable {:error :exited :code N} signal — recorded as skip (code 0) or fail
// (code != 0) — and the run continues.
//
// Counter-factual: pass two files, the first of which calls (exit 0) and the
// second of which fails an assertion. Without the trap the first file's exit
// ends the process with code 0 and the second never runs (gate is a false
// green). With the trap the first is recorded (skip) and the second runs and
// fails, so the run gates non-zero — proving the second file was reached.

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn test_calling_exit_does_not_truncate_the_run() {
    let dir = std::env::temp_dir().join("elle_exit_trap_test");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let a = dir.join("a-exits.lisp");
    let b = dir.join("b-fails.lisp");
    let db = dir.join("exit-trap.db");
    let _ = std::fs::remove_file(&db);
    // First file bails out with (exit 0) — the truncating idiom.
    std::fs::write(&a, "(exit 0)\n").unwrap();
    // Second file fails an assertion. If the runner reached it, the run gates
    // non-zero; if the first file's exit truncated the run, it never runs and
    // the gate is a false green (exit 0).
    std::fs::write(&b, "(assert false \"second file was reached\")\n").unwrap();

    let output = Command::new(elle_binary())
        .args(["test"])
        .arg(&a)
        .arg(&b)
        .arg("--db")
        .arg(&db)
        .arg("--timeout")
        .arg("30000")
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");

    assert_eq!(
        output.status.signal(),
        None,
        "elle test was killed by signal {:?}; stderr:\n{}",
        output.status.signal(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "a leading file calling (exit 0) truncated the run — the second file \
         (which fails an assertion) was never reached, so the gate is a false \
         green. Expected exit 1 (second file ran and failed); got {:?}. \
         stderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
