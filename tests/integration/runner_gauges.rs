// audited: 2026-09-20
// The runner reads three gauges of its own heap between files, so a per-file
// leak is a file name and a number rather than an OOM kill.
//
// docs/test-store.md
//
// The counter-factual these guard: a runner that samples nothing still passes
// every corpus test, because a leak in the harness is invisible to the harness.
// The `compile/dumps` leak of ~28000 regions per file lived behind a green
// suite until the machine ran out of memory.
//
// The trap in the chain test: two samples per file — one before, one after —
// would leave the rows written between them charged to nobody, and the gap is
// exactly where the runner's own work lives. One reading per boundary is what
// makes the readings chain, and the chain is what the assertion reads.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The fixtures a run processes, in the order they are given to the runner.
const FIXTURES: &[&str] = &["a-first.lisp", "b-second.lisp", "c-third.lisp"];

/// The gauges the runner reads about itself.
const KINDS: &[&str] = &["objects", "regions", "pages"];

/// Run `elle test` over every fixture in one process. Returns the runner's own
/// stderr, the session DB, and the scratch dir that owns both.
fn run_corpus(tag: &str) -> (String, std::path::PathBuf, crate::common::ScratchDir) {
    let dir = crate::common::ScratchDir::new(tag);
    let db = dir.join("s.db");

    let mut cmd = Command::new(elle_binary());
    cmd.args(["test"]);
    for name in FIXTURES {
        let path = dir.join(name);
        std::fs::write(&path, "(assert true \"ok\")\n").unwrap();
        cmd.arg(&path);
    }
    let out = cmd
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert!(
        out.status.success(),
        "a corpus of passing forms must gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (String::from_utf8_lossy(&out.stderr).into_owned(), db, dir)
}

/// Query `db` and return the rendered rows.
fn query(db: &std::path::Path, sql: &str) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Every file the run processed is charged on every gauge, once.
#[test]
fn every_file_a_run_processed_carries_one_row_per_gauge() {
    let (_stderr, db, _dir) = run_corpus("gauge-rows");

    let rows = query(
        &db,
        "SELECT kind AS kind, count(*) AS n, count(DISTINCT file) AS files \
         FROM gauge WHERE run_id = (SELECT max(id) FROM run) \
         GROUP BY kind ORDER BY kind",
    );
    for kind in KINDS {
        assert!(
            rows.contains(&format!(":kind \"{}\"", kind)),
            "the run must charge every file on the {} gauge, got:\n{}",
            kind,
            rows
        );
    }
    assert_eq!(
        rows.matches(&format!(":n {}", FIXTURES.len())).count(),
        KINDS.len(),
        "each gauge wants one row per file ({} of them), got:\n{}",
        FIXTURES.len(),
        rows
    );
    assert_eq!(
        rows.matches(&format!(":files {}", FIXTURES.len())).count(),
        KINDS.len(),
        "each gauge wants a row for a distinct file, got:\n{}",
        rows
    );
}

/// Consecutive boundary readings chain: a file's reading plus the next file's
/// delta is the next file's reading. Nothing the runner allocates between two
/// boundaries escapes being charged to a file.
#[test]
fn the_boundary_readings_chain_with_no_gap() {
    let (_stderr, db, _dir) = run_corpus("gauge-chain");

    let rows = query(
        &db,
        "SELECT count(*) AS pairs, \
         sum(CASE WHEN a.reading + b.delta = b.reading THEN 1 ELSE 0 END) AS closed \
         FROM gauge a JOIN gauge b \
         ON b.run_id = a.run_id AND b.kind = a.kind \
         AND b.id = (SELECT min(c.id) FROM gauge c \
         WHERE c.run_id = a.run_id AND c.kind = a.kind AND c.id > a.id) \
         WHERE a.run_id = (SELECT max(id) FROM run)",
    );
    // Three gauges, one boundary per file: two consecutive pairs each. Asserted
    // so the chain check cannot pass by finding no pairs to check.
    let pairs = KINDS.len() * (FIXTURES.len() - 1);
    assert!(
        rows.contains(&format!(":pairs {}", pairs)),
        "the chain wants {} consecutive pairs to check, got:\n{}",
        pairs,
        rows
    );
    assert!(
        rows.contains(&format!(":closed {}", pairs)),
        "every consecutive pair must chain — a break is a window charged to nobody, got:\n{}",
        rows
    );
}

/// The run says what it cost itself, and names the files it cost the most.
#[test]
fn the_summary_names_the_files_that_grew_the_runner_heap() {
    let (stderr, db, _dir) = run_corpus("gauge-summary");

    assert!(
        stderr.contains("runner heap"),
        "a run must report what it cost its own heap, got:\n{}",
        stderr
    );
    for kind in KINDS {
        assert!(
            stderr.contains(kind),
            "the runner-heap block must name the {} gauge, got:\n{}",
            kind,
            stderr
        );
    }
    for name in FIXTURES {
        assert!(
            stderr.contains(name),
            "the growers list must name {}, got:\n{}",
            name,
            stderr
        );
    }

    // --summary reads the same block back out of the DB, with nothing re-run.
    let s = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("summary");
    let s_err = String::from_utf8_lossy(&s.stderr);
    assert!(
        s_err.contains("runner heap") && s_err.contains(FIXTURES[0]),
        "--summary must render the growers of the run it reads, got:\n{}",
        s_err
    );
}
