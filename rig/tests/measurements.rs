// audited: 2026-10-04
// Under `elle-rig test` a run has the rig's build, and each reading is judged against that build's rows.
// docs/ratchet.md
// docs/test-store.md
//
// The counter-factual: a row with no :build belonged to every build that had
// none of its own, and away from its build a pin was a ceiling. A build that
// never read a pin was judged against another build's footprint, and a loose
// pin on it never went stale. Each test here writes its rows for the key this
// rig answers, so it holds on every box CI runs, and runs the rig as the
// runner from a scratch working directory that holds `tests/ledger`.

mod common;

use common::{stderr, Scratch};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn rig_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle-rig")
}

/// One line an `elle-rig -e` prints, trimmed.
fn eval(form: &str) -> String {
    let out = Command::new(rig_binary())
        .args(["-e", &format!("(println {form})")])
        .output()
        .expect("spawn elle-rig");
    assert!(
        out.status.success(),
        "`elle-rig -e {form}` failed:\n{}",
        stderr(&out)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The key this rig answers, which every row a test writes belongs to.
fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| eval("(elle/build)"))
}

/// The build a row with no `:build` belongs to.
fn reference() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| eval("(get ((import \"std/ratchet/ledger\")) :reference-build)"))
}

/// A build no box is.
const FOREIGN: &str = "interp-pool-plan9-mips";

/// `ROW` closed as a row of this rig's build: `["answer" :count 42` becomes
/// `["answer" :count 42 :build "KEY"]`.
fn own(row: &str) -> String {
    format!("{row} :build \"{}\"]", key())
}

/// `ROW` closed as a row of a build no box is.
fn foreign(row: &str) -> String {
    format!("{row} :build \"{FOREIGN}\"]")
}

/// `ROW` closed as `--repin` adopts it on this rig: with no `:build` on the
/// reference build, and naming this build anywhere else.
fn adopted(row: &str) -> String {
    if key() == reference() {
        format!("{row}]")
    } else {
        own(row)
    }
}

/// A producer of several forms, the whole-file shape the runner runs in a
/// worker under each JIT policy, that reads one number.
const PRODUCER: &str = "(def r ((import \"std/ratchet\")))\n\
                        (def answer 42)\n\
                        (r:read \"answer\" :count answer)\n";

/// A scratch working directory holding `producer.lisp`, its ledger under
/// `tests/ledger`, and a session DB of its own.
struct Bench {
    dir: Scratch,
}

impl Bench {
    /// `rows` is the producer's ledger, each row on a line of its own.
    fn new(tag: &str, rows: &[String], source: &str) -> Bench {
        let dir = Scratch::new(&format!("measure-{tag}"));
        dir.write("producer.lisp", source);
        std::fs::create_dir_all(dir.path().join("tests/ledger")).expect("create the ledger dir");
        dir.write(
            "tests/ledger/producer.lisp",
            &format!(
                "(elle/epoch 13)\n(producer \"producer.lisp\")\n{}\n",
                rows.join("\n")
            ),
        );
        Bench { dir }
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("s.db")
    }

    fn ledger(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("tests/ledger/producer.lisp"))
            .expect("read the ledger")
    }

    /// `elle-rig test ARGS producer.lisp` from the scratch directory.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(rig_binary())
            .arg("test")
            .args(args)
            .args(["--timeout", "60000"])
            .arg("--db")
            .arg(self.db())
            .arg("producer.lisp")
            .current_dir(self.dir.path())
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run elle-rig test")
    }

    /// The latest run's measurement rows.
    fn rows(&self) -> String {
        query(&self.db(), ROWS)
    }
}

/// `SQL` against the session DB at `db`, through the runner's `--query`.
fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(rig_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    assert!(
        out.status.success(),
        "the query failed: {sql}\n{}",
        stderr(&out)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The latest run's measurement rows, joined to the file that printed them.
const ROWS: &str = "SELECT f.file AS file, r.tier AS tier, m.subject AS subject, \
                    m.axis AS axis, m.value AS value, m.half AS half, m.unit AS unit, \
                    m.bound AS bound, m.kind AS kind, m.verdict AS verdict \
                    FROM measurement m JOIN result r ON r.id = m.result_id \
                    JOIN form f ON f.hash = r.form_hash \
                    WHERE m.run_id = (SELECT max(id) FROM run) ORDER BY r.tier";

#[test]
fn every_tier_lands_the_reading_as_a_judged_row() {
    let b = Bench::new("tiers", &[own("[\"answer\" :count 42")], PRODUCER);
    let out = b.run(&[]);
    assert!(
        out.status.success(),
        "a reading at its pin gates green:\n{}",
        stderr(&out)
    );

    let rows = b.rows();
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
    for want in [
        ":bound 42",
        ":kind \"pin\"",
        ":half 0",
        ":unit \"count\"",
        ":axis \"count\"",
    ] {
        assert!(rows.contains(want), "a row carries {want}, got:\n{rows}");
    }
    assert!(
        rows.contains(":file \"producer.lisp\""),
        "a row joins to the file that printed it, got:\n{rows}"
    );
}

#[test]
fn an_isolated_child_lands_its_readings_the_same_way() {
    // The child is `(elle/executable)`, this rig, so the run's build holds for
    // it as it holds for the runner.
    let b = Bench::new("child", &[own("[\"answer\" :count 42")], PRODUCER);
    let out = b.run(&["--isolate", ""]);
    assert!(out.status.success(), "the child passes:\n{}", stderr(&out));
    let rows = b.rows();
    assert!(
        rows.contains(":tier \"process\"") && rows.contains(":verdict \"ok\""),
        "the child's reading is a judged row on the process tier, got:\n{rows}"
    );
}

#[test]
fn the_run_row_records_the_build() {
    let b = Bench::new("build", &[own("[\"answer\" :count 42")], PRODUCER);
    b.run(&[]);
    let build = query(
        &b.db(),
        "SELECT build FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        build.contains(&format!(":build \"{}\"", key())),
        "the run row carries the rig's key {}, got:\n{build}",
        key()
    );
}

#[test]
fn a_worse_reading_is_a_regression_and_gates() {
    let b = Bench::new("worse", &[own("[\"answer\" :count 41")], PRODUCER);
    let out = b.run(&[]);
    assert!(
        !out.status.success(),
        "42 against a pin of 41 fails:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
    assert!(
        rows.contains(":verdict \"regression\"") && rows.contains(":bound 41"),
        "the row says regression against its bound, got:\n{rows}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("regression") && err.contains("answer") && err.contains("pinned 41"),
        "the summary names the verdict, the subject and the pin:\n{err}"
    );
}

#[test]
fn a_better_reading_is_stale_and_gates() {
    // A pin left loose is a ratchet that slipped: a later regression back to
    // 43 would pass it. Every pin is two-sided on its build, whichever build.
    let b = Bench::new("better", &[own("[\"answer\" :count 43")], PRODUCER);
    let out = b.run(&[]);
    assert!(
        !out.status.success(),
        "42 against a pin of 43 is stale:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
    assert!(
        rows.contains(":verdict \"stale\""),
        "the row says stale, got:\n{rows}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("stale") && err.contains("answer"),
        "the summary names the verdict and the subject:\n{err}"
    );
}

#[test]
fn rows_of_another_build_alone_leave_the_readings_unjudged() {
    // Another build's row would judge 42 a regression. Here it is no row at
    // all, and this build has none of its own: recorded, and nothing gates.
    let b = Bench::new("foreign", &[foreign("[\"answer\" :count 41")], PRODUCER);
    let out = b.run(&[]);
    assert!(
        out.status.success(),
        "a build with no rows of its own gates nothing:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
    assert!(
        rows.contains(":subject \"answer\"") && rows.contains(":verdict nil"),
        "the reading is recorded with no verdict, got:\n{rows}"
    );
    assert!(
        !rows.contains("missing"),
        "and another build's row is never missing here, got:\n{rows}"
    );
}

#[test]
fn a_reading_is_judged_against_its_own_builds_row() {
    let b = Bench::new(
        "both",
        &[
            foreign("[\"answer\" :count 41"),
            own("[\"answer\" :count 42"),
        ],
        PRODUCER,
    );
    let out = b.run(&[]);
    assert!(
        out.status.success(),
        "42 against this build's pin of 42 passes, whatever another build pins:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
    assert!(
        rows.contains(":bound 42") && !rows.contains(":bound 41"),
        "the row met is this build's, got:\n{rows}"
    );
}

#[test]
fn a_row_nobody_read_is_missing_and_gates() {
    // The counter-factual for elle-lisp/elle#1144: a probe deleted from a
    // dashboard left no trace. The row outlives the probe.
    let b = Bench::new(
        "unread",
        &[
            own("[\"answer\" :count 42"),
            own("[\"never read\" :count 1"),
        ],
        PRODUCER,
    );
    let out = b.run(&[]);
    assert!(
        !out.status.success(),
        "a row nobody read fails the gate:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
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
fn a_flat_live_growth_row_voids_its_axis() {
    // The trap: a gauge that reads flat and a shape that reclaims both read 0,
    // so a dead gauge paints every control green. The growth row is what
    // tells them apart, and a reading of 0 on the dead axis must not pass.
    let b = Bench::new(
        "flat",
        &[
            own("[\"widgets gauge (live-growth)\" :widgets :floor 0.5 :class :growth"),
            own("[\"thing\" :widgets 0"),
        ],
        "(def r ((import \"std/ratchet\")))\n\
         (def g (r:gauge :widgets \"widgets/op\" (fn [] 0) :disc (fn [j] j) :floor 0.5))\n\
         (r:delta \"thing\" (fn [] 1) :on [g] :n 10)\n",
    );
    let out = b.run(&[]);
    assert!(
        !out.status.success(),
        "a dead gauge fails the run:\n{}",
        stderr(&out)
    );
    let rows = b.rows();
    let n = rows.matches(":subject ").count();
    assert!(n >= 2, "both readings landed, got:\n{rows}");
    assert_eq!(
        rows.matches(":verdict \"void\"").count(),
        n,
        "the flat discriminator is void, and so is the reading of 0 on its \
         axis, got:\n{rows}"
    );
}

#[test]
fn repin_moves_a_stale_pin_and_keeps_the_comment_above_it() {
    // Moving a pin by hand is how a pin stays loose. The tool moves the token
    // and nothing else, so the comment survives.
    let b = Bench::new(
        "loose",
        &[
            "# the answer, pinned the day it was accepted".to_string(),
            own("[\"answer\" :count 43"),
        ],
        PRODUCER,
    );
    let out = b.run(&["--repin"]);
    let err = stderr(&out);
    assert!(
        err.contains("repin") && err.contains("answer") && err.contains("42"),
        "the tool prints the row it moved:\n{err}"
    );
    let text = b.ledger();
    assert!(
        text.contains(&own("[\"answer\" :count 42")),
        "the pin took the new reading, got:\n{text}"
    );
    assert!(
        text.contains("# the answer, pinned the day it was accepted\n"),
        "and the comment above the row survived, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "a run against the moved ledger gates green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_adopts_an_unledgered_reading_as_a_row() {
    let b = Bench::new(
        "adopt",
        &[own("[\"answer\" :count 42")],
        "(def r ((import \"std/ratchet\")))\n\
         (r:read \"answer\" :count 42)\n\
         (r:read \"extra\" :count 7)\n",
    );
    let out = b.run(&["--repin"]);
    assert!(
        stderr(&out).contains("extra"),
        "the tool prints the row it adopted:\n{}",
        stderr(&out)
    );
    let text = b.ledger();
    assert!(
        text.ends_with(&format!("{}\n", adopted("[\"extra\" :count 7"))),
        "the reading is a row of this build after the last one, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "and the adopted row judges the next run green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_adopts_a_growth_reading_as_a_growth_floor() {
    // The instrument's own live-growth reading carries its class and its
    // floor, so the adopted row is a floor and not a pin at 1.0.
    let b = Bench::new(
        "floor",
        &[own("[\"dropped\" :objects 0")],
        "(def r ((import \"std/ratchet\")))\n\
         (r:delta \"dropped\" (fn [] {:x 1}) :on [r:objects] :n 50)\n",
    );
    b.run(&["--repin"]);
    let text = b.ledger();
    assert!(
        text.contains(&adopted(
            "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth"
        )),
        "the discriminator is adopted as a growth floor, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "and the next run gates green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_refuses_a_regression_and_leaves_the_ledger_alone() {
    let b = Bench::new("refuse", &[own("[\"answer\" :count 41")], PRODUCER);
    let before = b.ledger();
    let out = b.run(&["--repin"]);
    assert!(
        !out.status.success(),
        "the regression still gates:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("refuse") && err.contains("answer"),
        "the tool says which row it refused to move:\n{err}"
    );
    assert_eq!(
        b.ledger(),
        before,
        "and the ledger is byte for byte what it was"
    );
}

#[test]
fn repin_on_a_build_with_no_rows_adopts_every_reading() {
    // How a build joins the ratchet: its first --repin writes a row of its
    // own for every reading, and another build's row is left as it was.
    let b = Bench::new("join", &[foreign("[\"answer\" :count 41")], PRODUCER);
    let out = b.run(&["--repin"]);
    assert!(
        out.status.success(),
        "a build with no rows gates nothing:\n{}",
        stderr(&out)
    );
    let text = b.ledger();
    assert!(
        text.contains(&adopted("[\"answer\" :count 42")),
        "the reading is adopted as a row of this build, got:\n{text}"
    );
    assert!(
        text.contains(&foreign("[\"answer\" :count 41")),
        "and the other build's row is untouched, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "the next run judges the adopted rows green:\n{}",
        stderr(&again)
    );
    assert!(
        b.rows().contains(":verdict \"ok\""),
        "against this build's rows:\n{}",
        b.rows()
    );
}

#[test]
fn a_run_with_a_host_has_no_build() {
    // `--host` runs each child on another program, which this rig cannot
    // speak for, so the run records nothing even on a rig.
    let b = Bench::new("host", &[own("[\"answer\" :count 41")], PRODUCER);
    let out = b.run(&["--isolate", "", "--host", rig_binary()]);
    assert!(
        out.status.success(),
        "a reading that would regress gates nothing with no build:\n{}",
        stderr(&out)
    );
    let count = query(&b.db(), "SELECT count(*) AS c FROM measurement");
    assert!(
        count.contains(":c 0"),
        "a run with --host writes no measurement row, got:\n{count}"
    );
}

/// The corpus fixture is a producer with a committed ledger, whose rows are
/// the reference build's: under the rig its readings are judged rows.
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "jit",
    feature = "uring",
    not(feature = "mlir"),
    not(feature = "wasm")
))]
#[test]
fn the_corpus_fixture_is_a_judged_producer() {
    let dir = Scratch::new("measure-fixture");
    let db = dir.path().join("s.db");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rig sits in the repository");
    let out = Command::new(rig_binary())
        .args(["test", "--timeout", "60000"])
        .arg("--db")
        .arg(&db)
        .arg("tests/impl/measure-channel.lisp")
        .current_dir(root)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run the fixture through the runner");
    assert!(
        out.status.success(),
        "the fixture gates green:\n{}",
        stderr(&out)
    );
    let rows = query(&db, ROWS);
    assert!(
        rows.contains(":subject \"channel-keep\"")
            && rows.contains(":axis \"objects\"")
            && rows.contains(":axis \"regions\""),
        "one probe, two gauges: one subject, two axes, got:\n{rows}"
    );
    assert!(
        rows.contains(":verdict \"ok\"")
            && !rows.contains(":verdict nil")
            && !rows.contains(":verdict \"unledgered\""),
        "every reading met a row in tests/ledger, got:\n{rows}"
    );
}
