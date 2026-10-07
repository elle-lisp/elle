// audited: 2026-10-05
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

use common::ratchet::*;
use common::stderr;
use std::process::Command;

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
fn the_summary_names_the_build() {
    // The tally says which readings were judged; the run line says as which
    // build, so a log read away from the store still says it.
    let b = Bench::new("summary", &[own("[\"answer\" :count 42")], PRODUCER);
    let out = b.run(&[]);
    let want = format!("· build {}", key());
    assert!(
        stderr(&out).contains(&want),
        "the post-run summary names the build:\n{}",
        stderr(&out)
    );
    let again = Command::new(rig_binary())
        .args(["test", "--summary", "--db"])
        .arg(b.db())
        .output()
        .expect("re-print the summary");
    assert!(
        stderr(&again).contains(&want),
        "and so does --summary:\n{}",
        stderr(&again)
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
    let dir = common::Scratch::new("measure-fixture");
    let db = dir.path().join("s.db");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
