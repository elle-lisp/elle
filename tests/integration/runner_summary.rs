// audited: 2026-10-15
// A run that passed prints its run line and its tally, and the run line ends with the run's wall time.
//
// docs/testing.md
//
// A suite pass prints one summary per batch, so every line a passing batch
// prints is repeated a few dozen times in one CI step. The counter-factual: a
// passing run printed two heap-gauge blocks after its tally, and a reader
// scrolled past them to find the one batch that failed.
//
// The trap in the time: `wall_ms` is written at completion, so a killed run
// has none, and a run line that printed `0.0 s` for it would read as a run
// that finished.

use crate::common::{query, scalar, ScratchDir};
use std::path::Path;
use std::process::{Command, Output};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// `elle test` over one file holding `src`, into the fresh store `db`.
fn run_one(dir: &ScratchDir, db: &Path, src: &str) -> Output {
    let file = dir.join("one.lisp");
    std::fs::write(&file, src).unwrap();
    Command::new(elle_binary())
        .args(["test"])
        .arg(&file)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
}

/// `elle test --summary` over `db`.
fn summary(db: &Path) -> Output {
    Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(db)
        .output()
        .expect("summary")
}

/// The non-empty lines a process wrote to stderr.
fn said(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// The latest run's `wall_ms`, which must be set.
fn wall_ms(db: &Path) -> i64 {
    let set = scalar(
        db,
        "SELECT (wall_ms IS NOT NULL) AS c FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert_eq!(set, 1, "a finished run must record its wall_ms");
    scalar(
        db,
        "SELECT wall_ms AS c FROM run WHERE id = (SELECT max(id) FROM run)",
    )
}

/// A wall time as the run line spells it: seconds, a point, and tenths.
fn seconds(ms: i64) -> String {
    format!("{}.{} s", ms / 1000, (ms / 100) % 10)
}

/// Whether `line` ends with a time in that spelling.
fn ends_with_a_time(line: &str) -> bool {
    let Some(rest) = line.strip_suffix(" s") else {
        return false;
    };
    let Some(word) = rest.rsplit(' ').next() else {
        return false;
    };
    let Some((whole, tenths)) = word.split_once('.') else {
        return false;
    };
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && tenths.len() == 1
        && tenths.chars().all(|c| c.is_ascii_digit())
}

#[test]
fn a_passing_run_prints_its_run_line_and_its_tally_and_nothing_else() {
    let dir = ScratchDir::new("summary-pass");
    let db = dir.join("s.db");
    let out = run_one(&dir, &db, "(assert true \"ok\")\n");
    let lines = said(&out);
    assert!(
        out.status.success(),
        "the run must gate green; stderr:\n{}",
        lines.join("\n")
    );
    assert_eq!(
        lines.len(),
        2,
        "a passing run prints its run line and its tally, got:\n{}",
        lines.join("\n")
    );
    assert!(
        lines[0].starts_with("elle test · run 1 of 1"),
        "the first line is the run line, got:\n{}",
        lines[0]
    );
    assert!(
        ends_with_a_time(&lines[0]),
        "the run line ends with the run's wall time, got:\n{}",
        lines[0]
    );
    // One result per tier the file ran on, so the count is the build's.
    assert!(
        lines[1].contains(" pass · ") && lines[1].ends_with(" timeout"),
        "the second line is the tally, got:\n{}",
        lines[1]
    );
}

#[test]
fn the_run_line_prints_the_wall_time_the_run_row_records() {
    let dir = ScratchDir::new("summary-wall");
    let db = dir.join("s.db");
    let out = run_one(&dir, &db, "(assert true \"ok\")\n");
    assert!(out.status.success(), "the run must gate green");

    let ms = wall_ms(&db);
    assert!(
        ms > 0,
        "a run that compiled and ran a file took time, got {ms} ms"
    );
    let line = &said(&out)[0];
    assert!(
        line.ends_with(&format!(" · {}", seconds(ms))),
        "the run line must print the recorded {ms} ms as {}, got:\n{line}",
        seconds(ms)
    );

    // --summary reads the time back out of the store, with nothing re-run.
    let again = said(&summary(&db));
    assert_eq!(
        again,
        said(&out),
        "--summary must re-print the summary the run printed"
    );
}

#[test]
fn a_failing_run_lists_its_problems_and_its_time() {
    let dir = ScratchDir::new("summary-fail");
    let db = dir.join("s.db");
    let out = run_one(&dir, &db, "(assert false \"the expected failure\")\n");
    let lines = said(&out);
    assert!(
        !out.status.success(),
        "the run must gate red; stderr:\n{}",
        lines.join("\n")
    );
    let run = lines
        .iter()
        .find(|line| line.starts_with("elle test · run "))
        .unwrap_or_else(|| panic!("no run line in:\n{}", lines.join("\n")));
    assert!(
        ends_with_a_time(run),
        "a failing run's line ends with its wall time too, got:\n{run}"
    );
    assert!(
        lines.iter().any(|line| line.contains("problem")),
        "a failing run says how many problems it found, got:\n{}",
        lines.join("\n")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("fail") && line.contains("one.lisp")),
        "a failing run lists the file that failed, got:\n{}",
        lines.join("\n")
    );
}

#[test]
fn a_killed_run_prints_no_time() {
    let dir = ScratchDir::new("summary-killed");
    let db = dir.join("s.db");
    let out = run_one(&dir, &db, "(os/sig-raise :sigkill)\n");
    assert!(!out.status.success(), "the fixture must kill the runner");
    let unset = query(
        &db,
        "SELECT (wall_ms IS NULL) AS unset FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        unset.contains(":unset 1"),
        "a run that never completed has no wall time, got:\n{unset}"
    );
    let lines = said(&summary(&db));
    let run = lines
        .iter()
        .find(|line| line.starts_with("elle test · run "))
        .unwrap_or_else(|| panic!("no run line in:\n{}", lines.join("\n")));
    assert!(
        !run.ends_with(" s"),
        "a killed run's line must not print a time it never recorded, got:\n{run}"
    );
}
