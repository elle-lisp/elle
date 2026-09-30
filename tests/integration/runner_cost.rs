// audited: 2026-09-30
// Each result records what it cost: wall and CPU time for a form in a worker,
// and a child's total, peak memory included, under --isolate.
//
// docs/test-store.md
//
// The counter-factual these guard: a runner that records no cost still passes
// every corpus test, and a store full of NULLs answers "which file is slow on
// this build" with nothing. A timeout that fails in one CI job and nowhere else
// is then a guess.

use std::path::Path;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `elle test` over one fixture holding `src`, into `db`, with `extra`
/// flags. Returns the runner's output; the caller decides what status it wants.
fn run(
    dir: &crate::common::ScratchDir,
    db: &Path,
    src: &str,
    extra: &[&str],
) -> std::process::Output {
    let path = dir.join("fixture.lisp");
    std::fs::write(&path, src).unwrap();
    Command::new(elle_binary())
        .arg("test")
        .arg(&path)
        .args(extra)
        .arg("--db")
        .arg(db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
}

/// Run and require a green gate.
fn run_green(dir: &crate::common::ScratchDir, db: &Path, src: &str, extra: &[&str]) {
    let out = run(dir, db, src, extra);
    assert!(
        out.status.success(),
        "the fixture must gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One integer a query renders under the alias `c`.
fn scalar(db: &Path, sql: &str) -> i64 {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    let rows = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "the query failed: {sql}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let at = rows
        .find(":c ")
        .unwrap_or_else(|| panic!("no `c` column in:\n{rows}"));
    let digits: String = rows[at + 3..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits
        .parse()
        .unwrap_or_else(|e| panic!("`c` is not a number ({e}) in:\n{rows}"))
}

/// How many of the latest run's rows satisfy `cond`, and how many there are.
fn latest(db: &Path, cond: &str) -> (i64, i64) {
    let run = "run_id = (SELECT max(id) FROM run)";
    (
        scalar(
            db,
            &format!("SELECT count(*) AS c FROM result WHERE {run} AND {cond}"),
        ),
        scalar(db, &format!("SELECT count(*) AS c FROM result WHERE {run}")),
    )
}

/// A multi-form file runs as one script per JIT policy. Each row's wall time
/// covers the sleep inside it, and each row carries a CPU time.
#[test]
fn a_script_records_its_wall_and_cpu_time_per_policy() {
    let dir = crate::common::ScratchDir::new("cost-script");
    let db = dir.join("s.db");
    run_green(&dir, &db, "(ev/sleep 0.3)\n(assert true \"slept\")\n", &[]);

    let (costed, rows) = latest(&db, "wall_ms >= 300 AND cpu_us IS NOT NULL");
    assert!(rows > 0, "the run recorded no result");
    assert_eq!(
        costed, rows,
        "every policy's row must record a wall time that covers the sleep and a CPU time"
    );
    let (no_rss, _) = latest(&db, "max_rss_kb IS NULL");
    assert_eq!(
        no_rss, rows,
        "a form in a worker records no peak resident set"
    );
}

/// A script that computes shows CPU time on its thread.
#[test]
fn a_busy_script_records_cpu_time() {
    let dir = crate::common::ScratchDir::new("cost-busy");
    let db = dir.join("s.db");
    run_green(
        &dir,
        &db,
        "(var i 0)\n(while (< i 2000000) (assign i (+ i 1)))\n(assert (= i 2000000) \"counted\")\n",
        &[],
    );

    let (busy, rows) = latest(&db, "cpu_us > 0");
    assert!(rows > 0, "the run recorded no result");
    assert_eq!(
        busy, rows,
        "every row of a busy script must count its CPU time"
    );
}

/// A single form runs on every tier the build carries, and each tier's row
/// records what it cost, a skip on an ineligible tier included.
#[test]
fn a_single_form_records_its_cost_on_every_tier() {
    let dir = crate::common::ScratchDir::new("cost-form");
    let db = dir.join("s.db");
    run_green(&dir, &db, "(assert (> 2 1) \"ordered\")\n", &[]);

    let (costed, rows) = latest(&db, "wall_ms IS NOT NULL AND cpu_us IS NOT NULL");
    assert!(rows > 0, "the run recorded no result");
    assert_eq!(
        costed, rows,
        "every tier's row must record wall and CPU time"
    );
}

/// A form that misses its deadline records the wall time it was given and no
/// CPU time: its worker never handed one back.
#[test]
fn a_timeout_records_its_wall_time_and_no_cpu_time() {
    let dir = crate::common::ScratchDir::new("cost-timeout");
    let db = dir.join("s.db");
    let out = run(
        &dir,
        &db,
        "(ev/sleep 5)\n(assert true \"never\")\n",
        &["--timeout", "500"],
    );
    assert!(!out.status.success(), "a timeout must gate non-zero");

    let (timed, rows) = latest(
        &db,
        "status = 'timeout' AND wall_ms >= 500 AND cpu_us IS NULL",
    );
    assert!(rows > 0, "the run recorded no result");
    assert_eq!(
        timed, rows,
        "every row must be a timeout that waited out its budget and holds no CPU time"
    );
}

/// A file that will not compile produced its row without running anything, so
/// the row holds no cost at all. The counter-factual is a zero, which reads as
/// a form that ran in no time. The fixture reads and fails to compile: a file
/// that does not read never reaches the file-level row.
#[test]
fn a_file_level_error_records_no_cost() {
    let dir = crate::common::ScratchDir::new("cost-file-error");
    let db = dir.join("s.db");
    let out = run(&dir, &db, "(assert (no-such-binding) \"unbound\")\n", &[]);
    assert!(
        !out.status.success(),
        "a file that does not compile must gate non-zero"
    );

    let (empty, rows) = latest(
        &db,
        "wall_ms IS NULL AND cpu_us IS NULL AND max_rss_kb IS NULL",
    );
    assert!(rows > 0, "the run recorded no result");
    assert_eq!(
        empty, rows,
        "a file-level error must leave every cost column NULL"
    );
}

/// An isolated child records its spawn-to-reap wall time, its CPU total and its
/// peak resident set, and an import of that store carries all three.
#[test]
fn an_isolated_child_records_its_total_and_an_import_keeps_it() {
    let dir = crate::common::ScratchDir::new("cost-isolate");
    let db = dir.join("s.db");
    run_green(
        &dir,
        &db,
        "(ev/sleep 0.3)\n(assert true \"slept\")\n",
        &["--isolate", ""],
    );

    let (costed, rows) = latest(
        &db,
        "tier = 'process' AND wall_ms >= 300 AND cpu_us > 0 AND max_rss_kb > 0",
    );
    assert_eq!(rows, 1, "one path under --isolate is one row");
    assert_eq!(
        costed, 1,
        "the child's row must record its wall time, CPU total and peak resident set"
    );

    let here = dir.join("here.db");
    let out = Command::new(elle_binary())
        .args(["test", "--import"])
        .arg(&db)
        .arg("--db")
        .arg(&here)
        .output()
        .expect("run elle test --import");
    assert!(
        out.status.success(),
        "the import failed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (kept, imported) = latest(&here, "max_rss_kb > 0 AND cpu_us > 0 AND wall_ms >= 300");
    assert_eq!(imported, 1, "the import must land the one result");
    assert_eq!(kept, 1, "the imported row must carry the child's cost");
}

/// A session DB whose `result` table predates `max_rss_kb` gains the column
/// when a run opens it. SQLite drops a column on request, which is how the
/// older table is made here.
#[test]
fn a_store_from_before_max_rss_kb_gains_the_column() {
    let dir = crate::common::ScratchDir::new("cost-migrate");
    let db = dir.join("s.db");
    let out = Command::new(elle_binary())
        .args([
            "test",
            "--query",
            "ALTER TABLE result DROP COLUMN max_rss_kb",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("drop the column");
    assert!(
        out.status.success(),
        "the store must have had a max_rss_kb column to drop; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    run_green(&dir, &db, "(assert true \"ok\")\n", &["--isolate", ""]);
    let (costed, rows) = latest(&db, "max_rss_kb > 0");
    assert_eq!(rows, 1, "one path under --isolate is one row");
    assert_eq!(costed, 1, "the migrated store must record the child's peak");
}
