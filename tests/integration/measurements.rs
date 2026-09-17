// audited: 2026-09-17
// A dashboard verdict is a row: what the measurement channel carries, what the
// runner writes, and what a direct run still does not write.
//
// docs/test-store.md
//
// The counter-factual: the estimator printed every rate to stdout and asserted
// on it, so a rate existed only as prose in a terminal. No query could ask what
// it was three commits ago, and the coverage of a dashboard could be checked
// against nothing. These read the rows that answer both.

use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The miniature dashboard: one probe read on the object count and the region
/// count, through the same estimator the leak dashboards use.
fn dashboard() -> PathBuf {
    repo_root().join("tests/elle/measure-channel.lisp")
}

/// Run the dashboard as an isolated child, which is what opens the channel.
fn isolate(db: &Path) -> std::process::Output {
    Command::new(elle_binary())
        .args(["test", "--isolate", "", "--timeout", "60000"])
        .arg("--db")
        .arg(db)
        .arg(dashboard())
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .env_remove("ELLE_TEST_MEASUREMENTS")
        .output()
        .expect("run elle test --isolate")
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

#[test]
fn a_dashboard_verdict_lands_in_the_measurement_table() {
    let dir = crate::common::ScratchDir::new("measure-rows");
    let db = dir.join("s.db");
    let out = isolate(&db);
    assert!(
        out.status.success(),
        "the dashboard passes, so the run gates green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let rows = query(
        &db,
        "SELECT subject AS subject, axis AS axis, value AS value, \
         unit AS unit, verdict AS verdict FROM measurement \
         WHERE run_id = (SELECT max(id) FROM run) ORDER BY axis",
    );

    // One probe, two gauges: one subject, two axes. The suffix the dashboard
    // displays (`channel-keep@regions`) is a rendering, so it must not reach
    // the subject column — otherwise no query can group the two readings.
    assert!(
        rows.contains(":subject \"channel-keep\""),
        "the subject is the probe's label without its display suffix, got:\n{rows}"
    );
    assert!(
        !rows.contains("channel-keep@regions"),
        "the display suffix is not part of the subject, got:\n{rows}"
    );
    assert!(
        rows.contains(":axis \"objects\"") && rows.contains(":axis \"regions\""),
        "each gauge names the dimension it read, got:\n{rows}"
    );
    assert!(
        rows.contains(":unit \"objects/op\"") && rows.contains(":unit \"regions/op\""),
        "and the unit a rate on it carries, got:\n{rows}"
    );
    // The probe keeps every object it makes, so both rates are ~1/op. A rate
    // recorded as 0 would be the dead-gauge reading, which is the one number
    // that must never pass for a measurement.
    assert!(
        rows.contains(":value 1.0"),
        "the measured rate is the value, got:\n{rows}"
    );
    // The probe is declared by-design, so it displays and records `growth`:
    // `open` in this table means a defect, exactly as on the dashboard.
    assert!(
        rows.contains(":verdict \"growth\"") && !rows.contains(":verdict \"open\""),
        "the recorded verdict is the displayed one, got:\n{rows}"
    );

    // A measurement belongs to the result that produced it, or nothing can say
    // which file a rate came from.
    let joined = query(
        &db,
        "SELECT f.file AS file FROM measurement m \
         JOIN result r ON r.id = m.result_id JOIN form f ON f.hash = r.form_hash \
         WHERE m.run_id = (SELECT max(id) FROM run)",
    );
    assert!(
        joined.contains("measure-channel.lisp"),
        "a measurement joins to the file that reported it, got:\n{joined}"
    );
}

#[test]
fn the_summary_names_the_measurements() {
    let dir = crate::common::ScratchDir::new("measure-summary");
    let db = dir.join("s.db");
    isolate(&db);

    let out = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("summary");
    let summary = String::from_utf8_lossy(&out.stderr);
    assert!(
        summary.contains("measurement"),
        "--summary must show the run's measurements, got:\n{summary}"
    );
    assert!(
        summary.contains("growth"),
        "and tally them by verdict, got:\n{summary}"
    );
}

/// The channel is a file the environment names, so a dashboard writes to it
/// only when something opened it. Unset, the dashboard prints and records
/// nothing — which is what keeps `elle tests/elle/oracle.lisp` reading as it
/// always did.
#[test]
fn the_channel_is_closed_unless_the_environment_opens_it() {
    let dir = crate::common::ScratchDir::new("measure-channel-env");
    let sink = dir.join("measurements.ndjson");

    let closed = Command::new(elle_binary())
        .arg(dashboard())
        .current_dir(repo_root())
        .env_remove("ELLE_TEST_MEASUREMENTS")
        .output()
        .expect("run the dashboard directly");
    assert!(
        closed.status.success(),
        "the dashboard passes on its own; stderr:\n{}",
        String::from_utf8_lossy(&closed.stderr)
    );
    let printed = String::from_utf8_lossy(&closed.stdout);
    assert!(
        printed.contains("channel-keep") && printed.contains("rate="),
        "stdout keeps the human rendering, got:\n{printed}"
    );
    assert!(
        !sink.exists(),
        "a closed channel writes nothing at {}",
        sink.display()
    );

    let opened = Command::new(elle_binary())
        .arg(dashboard())
        .current_dir(repo_root())
        .env("ELLE_TEST_MEASUREMENTS", &sink)
        .output()
        .expect("run the dashboard with the channel open");
    assert!(
        opened.status.success(),
        "opening the channel changes nothing about the run; stderr:\n{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    assert!(
        String::from_utf8_lossy(&opened.stdout).contains("rate="),
        "and stdout still carries the same rendering"
    );

    let written = std::fs::read_to_string(&sink)
        .unwrap_or_else(|e| panic!("read {}: {e}", sink.display()));
    let lines: Vec<&str> = written.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "one line per verdict the dashboard reported, got:\n{written}"
    );
    for want in [
        "\"subject\":\"channel-keep\"",
        "\"axis\":\"objects\"",
        "\"axis\":\"regions\"",
        "\"unit\":\"objects/op\"",
        "\"verdict\":\"growth\"",
    ] {
        assert!(
            written.contains(want),
            "the record must carry {want}, got:\n{written}"
        );
    }
}
