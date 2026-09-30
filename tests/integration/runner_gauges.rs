// audited: 2026-09-29
// The runner reads every heap gauge on its own heap and on its test code's heaps, per file.
//
// docs/test-gauges.md
//
// The counter-factual these guard: a runner that samples nothing still passes
// every corpus test, because a leak in the harness is invisible to the harness.
// The `compile/dumps` leak of ~28000 regions per file lived behind a green
// suite until the machine ran out of memory. The test heap has the same blind
// spot one level down: what the test code allocates never reaches the runner's
// heap, so only a reading taken inside the worker can see it.
//
// The trap in the chain test: two samples per file — one before, one after —
// would leave the rows written between them charged to nobody, and the gap is
// exactly where the runner's own work lives. One reading per boundary is what
// makes the readings chain, and the chain is what the assertion reads. Test-heap
// rows have no chain, so the chain query has to leave them out.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The fixtures a run processes, in the order they are given to the runner.
const FIXTURES: &[&str] = &["a-first.lisp", "b-second.lisp", "c-third.lisp"];

/// Every gauge, in the order the runner reads them.
const KINDS: &[&str] = &[
    "objects",
    "regions",
    "pages",
    "region-frees",
    "page-frees",
    "object-frees",
    "one-page-frees",
    "empty-frees",
    "one-object-frees",
    "few-object-frees",
    "many-object-frees",
    "adopts",
    "adopts-into-empty",
    "owned",
    "owned-frees",
    "owned-free-pages",
    "owned-free-objects",
    "owned-one-page-frees",
    "rescues",
    "rescue-survivors",
    "extracts",
    "reparents",
];

/// A one-form file whose form adopts: the pushed array joins the container's
/// owned subtree, and the container's release frees it.
const ADOPTS: &str = "(let [c (@array)]\n  (push c (array 1 2))\n  (length c))\n";

/// Run `elle test` over `files` (name, source) in one process, with `extra`
/// flags. Returns the runner's own stderr, the session DB, and the scratch dir
/// that owns both.
fn run_files(
    tag: &str,
    files: &[(&str, &str)],
    extra: &[&str],
) -> (String, std::path::PathBuf, crate::common::ScratchDir) {
    let dir = crate::common::ScratchDir::new(tag);
    let db = dir.join("s.db");

    let mut cmd = Command::new(elle_binary());
    cmd.args(["test"]);
    for (name, src) in files {
        let path = dir.join(name);
        std::fs::write(&path, src).unwrap();
        cmd.arg(&path);
    }
    let out = cmd
        .args(["--timeout", "30000"])
        .args(extra)
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

/// Every fixture, each a passing one-form file.
fn run_corpus(tag: &str) -> (String, std::path::PathBuf, crate::common::ScratchDir) {
    let files: Vec<(&str, &str)> = FIXTURES
        .iter()
        .map(|n| (*n, "(assert true \"ok\")\n"))
        .collect();
    run_files(tag, &files, &[])
}

/// Query `db` and return the rendered rows.
fn query(db: &std::path::Path, sql: &str) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    assert!(
        out.status.success(),
        "the query failed: {sql}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The per-kind row counts of one heap in the latest run.
fn rows_per_kind(db: &std::path::Path, heap: &str) -> String {
    query(
        db,
        &format!(
            "SELECT kind AS kind, count(*) AS n, count(DISTINCT file) AS files \
             FROM gauge WHERE run_id = (SELECT max(id) FROM run) AND heap = '{heap}' \
             GROUP BY kind ORDER BY kind"
        ),
    )
}

/// One file's summed delta on one heap and one gauge, in the latest run.
fn file_delta(db: &std::path::Path, heap: &str, file: &str, kind: &str) -> i64 {
    let rows = query(
        db,
        &format!(
            "SELECT coalesce(sum(delta), 0) AS c FROM gauge \
             WHERE run_id = (SELECT max(id) FROM run) AND heap = '{heap}' \
             AND kind = '{kind}' AND file LIKE '%{file}'"
        ),
    );
    let at = rows
        .find(":c ")
        .unwrap_or_else(|| panic!("no `c` column in:\n{rows}"));
    rows[at + 3..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect::<String>()
        .parse()
        .unwrap_or_else(|e| panic!("`c` is not a number ({e}) in:\n{rows}"))
}

/// Every file the run processed is charged on every gauge, once per heap.
#[test]
fn every_file_a_run_processed_carries_one_row_per_gauge_on_each_heap() {
    let (_stderr, db, _dir) = run_corpus("gauge-rows");

    for heap in ["runner", "test"] {
        let rows = rows_per_kind(&db, heap);
        for kind in KINDS {
            assert!(
                rows.contains(&format!(":kind \"{kind}\"")),
                "the run must charge every file on the {kind} gauge of the {heap} heap, \
                 got:\n{rows}"
            );
        }
        assert_eq!(
            rows.matches(&format!(":n {}", FIXTURES.len())).count(),
            KINDS.len(),
            "each {heap}-heap gauge wants one row per file ({} of them), got:\n{rows}",
            FIXTURES.len()
        );
        assert_eq!(
            rows.matches(&format!(":files {}", FIXTURES.len())).count(),
            KINDS.len(),
            "each {heap}-heap gauge wants a row for a distinct file, got:\n{rows}"
        );
    }
}

/// Consecutive runner-heap readings chain: a file's reading plus the next
/// file's delta is the next file's reading. Nothing the runner allocates
/// between two boundaries escapes being charged to a file.
#[test]
fn the_runner_heap_boundary_readings_chain_with_no_gap() {
    let (_stderr, db, _dir) = run_corpus("gauge-chain");

    let rows = query(
        &db,
        "SELECT count(*) AS pairs, \
         sum(CASE WHEN a.reading + b.delta = b.reading THEN 1 ELSE 0 END) AS closed \
         FROM gauge a JOIN gauge b \
         ON b.run_id = a.run_id AND b.kind = a.kind AND b.heap = a.heap \
         AND b.id = (SELECT min(c.id) FROM gauge c \
         WHERE c.run_id = a.run_id AND c.kind = a.kind AND c.heap = a.heap \
         AND c.id > a.id) \
         WHERE a.run_id = (SELECT max(id) FROM run) AND a.heap = 'runner'",
    );
    // Every gauge, one boundary per file: two consecutive pairs each. Asserted
    // so the chain check cannot pass by finding no pairs to check.
    let pairs = KINDS.len() * (FIXTURES.len() - 1);
    assert!(
        rows.contains(&format!(":pairs {pairs}")),
        "the chain wants {pairs} consecutive pairs to check, got:\n{rows}"
    );
    assert!(
        rows.contains(&format!(":closed {pairs}")),
        "every consecutive pair must chain — a break is a window charged to nobody, \
         got:\n{rows}"
    );
}

/// A test-heap row has no chain to join: the worker heap it measured is gone.
#[test]
fn a_test_heap_row_carries_no_reading() {
    let (_stderr, db, _dir) = run_corpus("gauge-test-reading");

    let rows = query(
        &db,
        "SELECT count(*) AS rows, count(reading) AS readings FROM gauge \
         WHERE run_id = (SELECT max(id) FROM run) AND heap = 'test'",
    );
    let expected = KINDS.len() * FIXTURES.len();
    assert!(
        rows.contains(&format!(":rows {expected}")) && rows.contains(":readings 0"),
        "every file wants a test-heap row per gauge, each with NULL reading, got:\n{rows}"
    );
}

/// The form's adoption lands on the test heap, where the form ran, and the
/// forest counters close over the file's summed deltas there.
#[test]
fn a_form_that_adopts_charges_the_adoption_to_the_test_heap() {
    let (_stderr, db, _dir) = run_files("gauge-adopts", &[("adopts.lisp", ADOPTS)], &[]);

    let adopts = file_delta(&db, "test", "adopts.lisp", "adopts");
    assert!(
        adopts >= 1,
        "each run of the form adopts once, so the test heap reads at least 1, got {adopts}"
    );
    let ended = ["owned-frees", "rescues", "extracts", "owned"]
        .iter()
        .map(|k| file_delta(&db, "test", "adopts.lisp", k))
        .sum::<i64>();
    assert_eq!(
        adopts, ended,
        "adopts must equal owned-frees + rescues + extracts + owned over the file's runs"
    );
}

/// An isolated child is a separate process, so its heap cannot be read: the
/// file records runner rows and no test rows.
#[test]
fn an_isolated_file_records_no_test_heap_rows() {
    let (_stderr, db, _dir) = run_files(
        "gauge-isolated",
        &[("iso.lisp", "(assert true \"ok\")\n")],
        &["--isolate", ""],
    );

    let rows = query(
        &db,
        "SELECT sum(CASE WHEN heap = 'runner' THEN 1 ELSE 0 END) AS runner, \
         sum(CASE WHEN heap = 'test' THEN 1 ELSE 0 END) AS test \
         FROM gauge WHERE run_id = (SELECT max(id) FROM run)",
    );
    assert!(
        rows.contains(&format!(":runner {}", KINDS.len())) && rows.contains(":test 0"),
        "an isolated file wants one runner row per gauge and no test row, got:\n{rows}"
    );
}

/// The run says what each file cost each heap, and names the files that cost
/// the most.
#[test]
fn the_summary_names_the_files_that_grew_each_heap() {
    let (stderr, db, _dir) = run_corpus("gauge-summary");

    for block in ["runner heap", "test heap"] {
        assert!(
            stderr.contains(block),
            "a run must report a {block} block, got:\n{stderr}"
        );
    }
    for kind in KINDS {
        assert!(
            stderr.contains(&format!(" {kind} ")),
            "the heap blocks must name the {kind} gauge, got:\n{stderr}"
        );
    }
    for name in FIXTURES {
        assert!(
            stderr.contains(name),
            "the growers list must name {name}, got:\n{stderr}"
        );
    }

    // --summary reads the same blocks back out of the DB, with nothing re-run.
    let s = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("summary");
    let s_err = String::from_utf8_lossy(&s.stderr);
    assert!(
        s_err.contains("runner heap") && s_err.contains("test heap") && s_err.contains(FIXTURES[0]),
        "--summary must render the growers of the run it reads, got:\n{s_err}"
    );
}
