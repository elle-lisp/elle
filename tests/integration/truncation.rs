// Gate honesty: a killed `elle test` run must be readable as killed — never as
// an all-pass run with zero counters. The OOM killer is the real-world shape
// (the whole corpus in one process can exceed the machine); the fixture below
// reproduces it deterministically by SIGKILLing the runner from inside a test
// form. See docs/test-runner.md § Run honesty.

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A completed run stamps `finished_at`, records how many files it planned
/// (`n_selected`), and aggregates the counters.
#[test]
fn completed_run_stamps_finished_at_and_counters() {
    let dir = crate::common::ScratchDir::new("truncation-done");
    let pass = dir.join("pass.lisp");
    std::fs::write(&pass, "(assert true \"ok\")\n").unwrap();
    let db = dir.join("s.db");

    let out = Command::new(elle_binary())
        .args(["test"])
        .arg(&pass)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert!(
        out.status.success(),
        "trivial run should gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let q = Command::new(elle_binary())
        .args([
            "test",
            "--query",
            "SELECT (finished_at IS NOT NULL) AS done, n_selected AS sel, \
             (n_pass > 0) AS haspass FROM run WHERE id = (SELECT max(id) FROM run)",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("query run row");
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        stdout.contains(":done 1"),
        "completed run must stamp finished_at, got: {}",
        stdout
    );
    assert!(
        stdout.contains(":sel 1"),
        "run row must record the planned file count, got: {}",
        stdout
    );
    assert!(
        stdout.contains(":haspass 1"),
        "completed run must aggregate counters, got: {}",
        stdout
    );
}

/// A run killed mid-corpus leaves finished_at NULL; --summary labels it
/// DID NOT COMPLETE (with the live partial tally, not the zero stored
/// counters), and the next run against the same DB warns about it.
#[test]
fn killed_run_reads_as_truncated_not_green() {
    let dir = crate::common::ScratchDir::new("truncation-kill");
    let a = dir.join("a-passes.lisp");
    let b = dir.join("b-kills.lisp");
    let c = dir.join("c-never-runs.lisp");
    let db = dir.join("s.db");
    std::fs::write(&a, "(assert true \"ok\")\n").unwrap();
    // SIGKILL the runner process itself — the deterministic OOM-killer stand-in.
    std::fs::write(&b, "(os/sig-raise :sigkill)\n").unwrap();
    std::fs::write(&c, "(assert true \"ok\")\n").unwrap();

    let out = Command::new(elle_binary())
        .args(["test"])
        .arg(&a)
        .arg(&b)
        .arg(&c)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert_eq!(
        out.status.signal(),
        Some(9),
        "the fixture must SIGKILL the runner; got code {:?}, stderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    // --summary must say the run was truncated, not report it as 0-fail green.
    let s = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("summary");
    let s_err = String::from_utf8_lossy(&s.stderr);
    assert!(
        s_err.contains("DID NOT COMPLETE"),
        "--summary must label the killed run truncated, got:\n{}",
        s_err
    );
    assert!(
        s_err.contains("of 3 selected"),
        "--summary must say how much of the selection was reached, got:\n{}",
        s_err
    );

    // The next run against the same session DB warns about its predecessor —
    // and itself completes and gates green.
    let n = Command::new(elle_binary())
        .args(["test"])
        .arg(&c)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("next run");
    let n_err = String::from_utf8_lossy(&n.stderr);
    assert!(
        n.status.success(),
        "follow-up run should gate green; stderr:\n{}",
        n_err
    );
    assert!(
        n_err.contains("DID NOT COMPLETE"),
        "the next run must warn that its predecessor was killed, got:\n{}",
        n_err
    );
}
