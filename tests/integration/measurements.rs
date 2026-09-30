// audited: 2026-09-30
// A reading a producer prints is a row: read out of every captured stdout,
// judged against the file's ledger, and gated on.
//
// docs/test-store.md
// docs/ratchet.md
//
// The counter-factual: the channel was a file an environment variable named,
// opened for an isolated child alone, so a corpus file in a worker thread
// printed its rates and recorded nothing — and every verdict recorded was the
// dashboard's own, compared against nothing committed. These drive a scratch
// producer through the runner against a scratch ledger and read the rows.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A producer of several forms — the whole-file shape the runner runs in a
/// worker under each JIT policy — that reads one number and reports.
const PRODUCER: &str = "(def r ((import \"std/ratchet\")))\n\
                        (def answer 42)\n\
                        (r:read \"answer\" :count answer)\n\
                        (r:report)\n";

/// A scratch producer, its ledger directory, and a session DB of its own.
struct Bench {
    dir: crate::common::ScratchDir,
    producer: PathBuf,
}

impl Bench {
    /// `rows` is the producer's ledger, or None for a producer with no ledger.
    fn new(tag: &str, rows: Option<&str>) -> Bench {
        let dir = crate::common::ScratchDir::new(&format!("measure-{tag}"));
        let producer = dir.join("producer.lisp");
        std::fs::write(&producer, PRODUCER).expect("write the producer");
        let ledger = dir.join("ledger");
        std::fs::create_dir_all(&ledger).expect("create the ledger dir");
        if let Some(rows) = rows {
            std::fs::write(
                ledger.join("producer.lisp"),
                format!(
                    "(elle/epoch 13)\n(producer \"{}\")\n{rows}\n",
                    producer.to_str().expect("utf-8 path")
                ),
            )
            .expect("write the ledger");
        }
        Bench { dir, producer }
    }

    fn db(&self) -> PathBuf {
        self.dir.join("s.db")
    }

    /// `elle test ARGS producer` against the scratch ledger and DB.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(elle_binary())
            .arg("test")
            .args(args)
            .args(["--timeout", "60000"])
            .arg("--db")
            .arg(self.db())
            .arg(&self.producer)
            .current_dir(repo_root())
            .env("ELLE_LEDGER", self.dir.join("ledger"))
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run elle test")
    }

    fn summary(&self) -> String {
        let out = Command::new(elle_binary())
            .args(["test", "--summary"])
            .arg("--db")
            .arg(self.db())
            .output()
            .expect("summary");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }
}

fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The latest run's measurement rows, joined to the file that printed them.
const ROWS: &str = "SELECT f.file AS file, r.tier AS tier, m.subject AS subject, \
                    m.axis AS axis, m.value AS value, m.half AS half, m.unit AS unit, \
                    m.bound AS bound, m.kind AS kind, m.verdict AS verdict \
                    FROM measurement m JOIN result r ON r.id = m.result_id \
                    JOIN form f ON f.hash = r.form_hash \
                    WHERE m.run_id = (SELECT max(id) FROM run) ORDER BY r.tier";

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn every_tier_lands_the_reading_as_a_judged_row() {
    let b = Bench::new("tiers", Some("[\"answer\" :count 42]"));
    let out = b.run(&[]);
    assert!(out.status.success(), "a reading at its pin gates green:\n{}", stderr(&out));

    let rows = query(&b.db(), ROWS);
    let n = rows.matches(":subject \"answer\"").count();
    assert!(n >= 1, "the reading lands as a row per tier, got:\n{rows}");
    assert!(
        rows.contains(":tier \"vm\""),
        "the bytecode policy's run printed it, got:\n{rows}"
    );
    assert_eq!(
        rows.matches(":verdict \"ok\"").count(),
        n,
        "every row is judged ok against the pin, got:\n{rows}"
    );
    for want in [":bound 42", ":kind \"pin\"", ":half 0", ":unit \"count\"", ":axis \"count\""] {
        assert!(rows.contains(want), "a row carries {want}, got:\n{rows}");
    }
    assert!(
        rows.contains(b.producer.to_str().expect("utf-8 path")),
        "a row joins to the file that printed it, got:\n{rows}"
    );
}

#[test]
fn an_isolated_child_lands_its_readings_the_same_way() {
    let b = Bench::new("child", Some("[\"answer\" :count 42]"));
    let out = b.run(&["--isolate", ""]);
    assert!(out.status.success(), "the child passes:\n{}", stderr(&out));

    let rows = query(&b.db(), ROWS);
    assert!(
        rows.contains(":tier \"process\"") && rows.contains(":verdict \"ok\""),
        "the child's reading is a judged row on the process tier, got:\n{rows}"
    );
}

#[test]
fn a_row_the_producer_never_read_is_missing_and_gates() {
    // The counter-factual for elle-lisp/elle#1144: a probe deleted from a
    // dashboard left no trace. The row outlives the probe.
    let b = Bench::new(
        "unread",
        Some("[\"answer\" :count 42]\n[\"never read\" :count 1]"),
    );
    let out = b.run(&[]);
    assert!(!out.status.success(), "a row nobody read fails the gate:\n{}", stderr(&out));

    let rows = query(&b.db(), ROWS);
    assert!(
        rows.contains(":subject \"never read\"") && rows.contains(":verdict \"missing\""),
        "the unread row is recorded as missing, got:\n{rows}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("missing") && err.contains("never read"),
        "the summary names it:\n{err}"
    );
}

#[test]
fn a_regression_gates_the_run_and_the_summary_names_it() {
    let b = Bench::new("worse", Some("[\"answer\" :count 41]"));
    let out = b.run(&[]);
    assert!(!out.status.success(), "42 against a pin of 41 fails:\n{}", stderr(&out));

    let rows = query(&b.db(), ROWS);
    assert!(
        rows.contains(":verdict \"regression\"") && rows.contains(":bound 41"),
        "the row says regression against its bound, got:\n{rows}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("regression") && err.contains("answer") && err.contains("pinned 41"),
        "the summary names the verdict, the subject and the pin:\n{err}"
    );
    assert!(
        b.summary().contains("regression"),
        "--summary reads the same verdicts back out of the DB"
    );
}

#[test]
fn a_producer_with_no_ledger_is_recorded_and_not_judged() {
    // The migration rule: a dashboard keeps its history in the table before
    // its ledger exists, and the gate leaves it alone until a row names it.
    let b = Bench::new("noledger", None);
    let out = b.run(&[]);
    assert!(out.status.success(), "nothing to judge, nothing to fail:\n{}", stderr(&out));

    let rows = query(&b.db(), ROWS);
    assert!(
        rows.contains(":subject \"answer\"") && rows.contains(":verdict nil"),
        "the reading is a row with no verdict, got:\n{rows}"
    );
    assert!(
        stderr(&out).contains("unjudged"),
        "the summary says how many readings met no ledger:\n{}",
        stderr(&out)
    );
}

#[test]
fn the_summary_tallies_readings_by_verdict() {
    let b = Bench::new("tally", Some("[\"answer\" :count 42]"));
    let out = b.run(&[]);
    let err = stderr(&out);
    assert!(
        err.contains("reading") && err.contains("ok"),
        "a run that recorded readings says so, by verdict:\n{err}"
    );
}

/// The corpus fixture is a real producer with a real ledger: under the runner
/// its readings are judged rows, and run directly it judges itself.
#[test]
fn the_corpus_fixture_is_a_ledgered_producer() {
    let dir = crate::common::ScratchDir::new("measure-fixture");
    let db = dir.join("s.db");
    let out = Command::new(elle_binary())
        .args(["test", "--timeout", "60000"])
        .arg("--db")
        .arg(&db)
        .arg("tests/impl/measure-channel.lisp")
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .env_remove("ELLE_LEDGER")
        .output()
        .expect("run the fixture through the runner");
    assert!(out.status.success(), "the fixture gates green:\n{}", stderr(&out));
    let rows = query(&db, ROWS);
    assert!(
        rows.contains(":subject \"channel-keep\"")
            && rows.contains(":axis \"objects\"")
            && rows.contains(":axis \"regions\""),
        "one probe, two gauges: one subject, two axes, got:\n{rows}"
    );
    assert!(
        !rows.contains(":verdict nil") && !rows.contains(":verdict \"unledgered\""),
        "every reading met a row in tests/ledger, got:\n{rows}"
    );

    let direct = Command::new(elle_binary())
        .arg("tests/impl/measure-channel.lisp")
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .env_remove("ELLE_LEDGER")
        .output()
        .expect("run the fixture directly");
    assert!(direct.status.success(), "run directly it judges itself:\n{}", stderr(&direct));
    let printed = String::from_utf8_lossy(&direct.stdout);
    assert!(
        printed.contains("measure {") && printed.contains("\"verdict\":\"ok\""),
        "and prints each reading with its verdict:\n{printed}"
    );
}
