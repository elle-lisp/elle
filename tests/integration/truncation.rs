// audited: 2026-09-29
// A run that did not finish reads as killed or as still running, never as green.
//
// docs/test-runner.md
//
// The OOM killer is the real-world kill (the whole corpus in one process can
// exceed the machine); the fixtures below reproduce it deterministically by
// SIGKILLing the runner from inside a test form.
//
// The trap: a live run and a killed run leave the same row, `finished_at` NULL.
// Two batches sharing one session DB each warned that the other was killed, on
// every batch, while both were healthy.

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A runner that is still working: `elle test` over one form that sleeps far
/// longer than any assertion below takes. Dropping it kills it, so a failed
/// assertion leaves no runner behind.
struct LiveRun {
    child: Child,
}

impl LiveRun {
    fn start(dir: &crate::common::ScratchDir, db: &Path) -> LiveRun {
        let sleeper = dir.join("sleeps.lisp");
        std::fs::write(&sleeper, "(ev/sleep 120)\n").unwrap();
        let child = Command::new(elle_binary())
            .args(["test"])
            .arg(&sleeper)
            .args(["--timeout", "240000"])
            .arg("--db")
            .arg(db)
            .env_remove("RUST_MIN_STACK")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the live runner");
        let live = LiveRun { child };
        live.wait_for_row(db);
        live
    }

    /// Block until the live runner has written its run row. The row lands at
    /// insert, a moment after the process starts, and every check below reads
    /// that row.
    fn wait_for_row(&self, db: &Path) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if query(db, "SELECT count(*) AS c FROM run").contains(":c 1") {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the live runner wrote no run row within 60 s");
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The rendered rows of `sql` against `db`.
fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn summary(db: &Path) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(db)
        .output()
        .expect("summary");
    String::from_utf8_lossy(&out.stderr).into_owned()
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

/// A run that is still working is labelled as running, with its pid, and a
/// kill of that same run is then labelled killed. The second half is the
/// counter-factual for the first: a view that said "running" of every
/// unfinished row would hide the kill the first test pins.
#[test]
fn a_run_in_flight_reads_as_running_until_it_is_killed() {
    let dir = crate::common::ScratchDir::new("truncation-live");
    let db = dir.join("s.db");
    let mut live = LiveRun::start(&dir, &db);

    let pid = query(&db, "SELECT pid AS pid FROM run WHERE finished_at IS NULL");
    assert!(
        pid.contains(&format!(":pid {}", live.child.id())),
        "the run row must carry the runner's own pid ({}), got:\n{pid}",
        live.child.id()
    );

    let running = summary(&db);
    assert!(
        running.contains("STILL RUNNING") && running.contains(&live.child.id().to_string()),
        "--summary must label a live run as running and name its pid, got:\n{running}"
    );
    assert!(
        !running.contains("DID NOT COMPLETE"),
        "--summary must not call a live run killed, got:\n{running}"
    );

    live.kill();
    let killed = summary(&db);
    assert!(
        killed.contains("DID NOT COMPLETE"),
        "once its process is gone the same run must read as killed, got:\n{killed}"
    );
}

/// Two runs share a session DB whenever two batches or two worktrees run at
/// once. The second one names the first as running, not as killed, and still
/// gates on its own results.
#[test]
fn the_next_run_names_a_live_predecessor_as_running() {
    let dir = crate::common::ScratchDir::new("truncation-sibling");
    let db = dir.join("s.db");
    let live = LiveRun::start(&dir, &db);

    let quick = dir.join("quick.lisp");
    std::fs::write(&quick, "(assert true \"ok\")\n").unwrap();
    let next = Command::new(elle_binary())
        .args(["test"])
        .arg(&quick)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("next run");
    let said = String::from_utf8_lossy(&next.stderr);
    assert!(
        next.status.success(),
        "a run beside a live sibling should gate green; stderr:\n{said}"
    );
    assert!(
        said.contains("still running") && said.contains(&live.child.id().to_string()),
        "the next run must say its predecessor is still running and name its pid, got:\n{said}"
    );
    assert!(
        !said.contains("was killed") && !said.contains("DID NOT COMPLETE"),
        "the next run must not call a live predecessor killed, got:\n{said}"
    );
}
