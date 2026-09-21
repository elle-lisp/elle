// audited: 2026-09-21
// A run row names the binary that produced it, so a verdict belongs to a
// build and not merely to a commit.
//
// docs/test-store.md
//
// The counter-factual: a result recorded the form and the commit and nothing
// about the executable. Two runs of two compilers read identically, no cached
// green could prove which binary earned it, and archaeology across a compiler
// change had nothing to group by.
//
// The trap: the fingerprint has to be the binary's own answer. A column filled
// from the version string would agree across every build of one release, which
// is exactly the case a cache must not reuse.

use std::io::Write;
use std::path::Path;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `elle test` over one trivial passing form against `db`.
fn run_once(dir: &crate::common::ScratchDir, name: &str, db: &Path) {
    let fixture = dir.join(name);
    std::fs::write(&fixture, "(assert true \"ok\")\n").expect("write fixture");
    let out = Command::new(elle_binary())
        .args(["test"])
        .arg(&fixture)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert!(
        out.status.success(),
        "a passing form gates green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
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

#[test]
fn a_run_row_carries_the_fingerprint_of_the_binary_that_ran_it() {
    let dir = crate::common::ScratchDir::new("boot-fingerprint-row");
    let db = dir.join("s.db");
    run_once(&dir, "pass.lisp", &db);

    // The column holds the hash as a number. Nothing displays a fingerprint,
    // and what reads it compares, groups and joins it — a text column would
    // cost twice the bytes and compare a character at a time.
    let rows = query(
        &db,
        "SELECT typeof(boot_fingerprint) AS kind, (boot_fingerprint IS NULL) AS none \
         FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        rows.contains(":kind \"integer\""),
        "the fingerprint is a 64-bit number, not text, got:\n{rows}"
    );
    assert!(
        rows.contains(":none 0"),
        "this box names its own binary, so the run recorded one, got:\n{rows}"
    );
}

/// One binary, one answer. A fingerprint that moved between two runs of the
/// same executable could key nothing.
#[test]
fn two_runs_of_one_binary_record_one_fingerprint() {
    let dir = crate::common::ScratchDir::new("boot-fingerprint-stable");
    let db = dir.join("s.db");
    run_once(&dir, "first.lisp", &db);
    run_once(&dir, "second.lisp", &db);

    let rows = query(
        &db,
        "SELECT count(*) AS runs, count(DISTINCT boot_fingerprint) AS prints FROM run",
    );
    assert!(
        rows.contains(":runs 2") && rows.contains(":prints 1"),
        "two runs of one binary share one fingerprint, got:\n{rows}"
    );
}

/// The column is what the binary reports about itself, and the binary reports
/// it to any program that asks.
#[test]
fn the_binary_reports_the_fingerprint_the_run_recorded() {
    let dir = crate::common::ScratchDir::new("boot-fingerprint-primitive");
    let db = dir.join("s.db");
    run_once(&dir, "pass.lisp", &db);

    let mut child = Command::new(elle_binary())
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"(print (elle/boot-fingerprint))")
        .expect("write the program");
    let out = child.wait_with_output().expect("wait for elle");
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        out.status.success() && printed.parse::<i64>().is_ok(),
        "(elle/boot-fingerprint) answers with a number, got {printed:?}; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let rows = query(
        &db,
        "SELECT boot_fingerprint AS fp FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        rows.contains(&format!(":fp {printed}")),
        "the run recorded what the binary reports ({printed}), got:\n{rows}"
    );
}
