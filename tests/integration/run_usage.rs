// audited: 2026-11-22
// A finished run records what the runner's process cost; a killed run records
// none, an older store gains the columns, and an import keeps them.
//
// docs/test-store.md
//
// The counter-factual these guard: a runner that reads its cost from the
// calling thread alone, as `clock/cpu` would, misses every worker and the JIT's
// thread, and reads a few milliseconds for a run that burned seconds. A runner
// that stamped the cost at insert would give a killed run a peak, and the
// killed run would read as one that measured itself.

use crate::common::{elle_test, query, scalar, ScratchDir};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;

/// The latest run's row, filtered by `cond`: 1 when it holds, 0 when not.
fn latest_run(db: &Path, cond: &str) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT count(*) AS c FROM run r \
             WHERE r.id = (SELECT max(id) FROM run) AND {cond}"
        ),
    )
}

/// Write `src` as a fixture in `dir`, run it into `db`, and require a green gate.
fn run_green(dir: &ScratchDir, db: &Path, src: &str) {
    let path = dir.join("fixture.lisp");
    std::fs::write(&path, src).unwrap();
    let out = elle_test(db, [&path]);
    assert!(
        out.status.success(),
        "the fixture must gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A finished run records its process's peak resident set and its CPU time.
/// Booting a runtime alone costs user time, so a zero is a reading that was
/// never taken.
#[test]
fn a_finished_run_records_what_its_process_cost() {
    let dir = ScratchDir::new("usage-done");
    let db = dir.join("s.db");
    run_green(&dir, &db, "(assert true \"ok\")\n");
    assert_eq!(
        latest_run(
            &db,
            "r.max_rss_kb > 0 AND r.cpu_user_ms > 0 AND r.cpu_sys_ms >= 0"
        ),
        1,
        "the run row must carry a peak and both CPU times:\n{}",
        query(
            &db,
            "SELECT max_rss_kb, cpu_user_ms, cpu_sys_ms FROM run ORDER BY id DESC LIMIT 1"
        )
    );
}

/// The run's CPU time covers the workers that ran its forms. A multi-form file
/// runs once per JIT policy, each run in a worker that spins for 0.3 s of its
/// own CPU time, so the results alone hold at least 0.3 s. Each column is
/// truncated to whole milliseconds, which the 2 ms of slack absorbs.
#[test]
fn the_run_counts_the_cpu_time_of_its_workers() {
    let dir = ScratchDir::new("usage-workers");
    let db = dir.join("s.db");
    run_green(
        &dir,
        &db,
        "(def t0 (clock/cpu))\n\
         (while (< (- (clock/cpu) t0) 0.3) nil)\n\
         (assert true \"spun\")\n",
    );
    let workers = "(SELECT sum(cpu_us) FROM result WHERE run_id = r.id)";
    assert_eq!(
        latest_run(
            &db,
            &format!(
                "{workers} >= 300000 \
                 AND (r.cpu_user_ms + r.cpu_sys_ms + 2) * 1000 >= {workers}"
            )
        ),
        1,
        "the run's CPU time must cover every worker's:\n{}",
        query(
            &db,
            "SELECT r.cpu_user_ms, r.cpu_sys_ms, \
             (SELECT sum(cpu_us) FROM result WHERE run_id = r.id) AS workers \
             FROM run r ORDER BY r.id DESC LIMIT 1"
        )
    );
}

/// A run the kernel kills never reaches its completion stamp, so it records no
/// cost. The fixture SIGKILLs the runner, which stands in for the OOM killer.
#[test]
fn a_killed_run_records_no_cost() {
    let dir = ScratchDir::new("usage-killed");
    let db = dir.join("s.db");
    let path = dir.join("kills.lisp");
    std::fs::write(&path, "(os/sig-raise :sigkill)\n").unwrap();
    let out = elle_test(&db, [&path]);
    assert_eq!(
        out.status.signal(),
        Some(9),
        "the fixture must SIGKILL the runner; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        latest_run(
            &db,
            "r.finished_at IS NULL AND r.max_rss_kb IS NULL \
             AND r.cpu_user_ms IS NULL AND r.cpu_sys_ms IS NULL"
        ),
        1,
        "a killed run must leave all three cost columns NULL"
    );
}

/// A session DB whose `run` table predates the cost columns gains them when a
/// run opens it. SQLite drops a column on request, which is how the older
/// table is made here.
#[test]
fn a_store_from_before_the_cost_columns_gains_them() {
    let dir = ScratchDir::new("usage-migrate");
    let db = dir.join("s.db");
    for col in ["max_rss_kb", "cpu_user_ms", "cpu_sys_ms"] {
        query(&db, &format!("ALTER TABLE run DROP COLUMN {col}"));
    }
    run_green(&dir, &db, "(assert true \"ok\")\n");
    assert_eq!(
        latest_run(&db, "r.max_rss_kb > 0 AND r.cpu_user_ms > 0"),
        1,
        "the migrated store must record the run's cost"
    );
}

/// An import carries a run's cost into the store it lands in.
#[test]
fn an_import_keeps_the_runs_cost() {
    let dir = ScratchDir::new("usage-import");
    let db = dir.join("s.db");
    run_green(&dir, &db, "(assert true \"ok\")\n");

    let here = dir.join("here.db");
    let out = elle_test(&here, [std::ffi::OsStr::new("--import"), db.as_os_str()]);
    assert!(
        out.status.success(),
        "the import failed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        latest_run(&here, "r.max_rss_kb > 0 AND r.cpu_user_ms > 0"),
        1,
        "the imported run must carry its cost"
    );
}
