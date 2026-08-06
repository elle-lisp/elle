// A form killed by the per-test deadline keeps what it printed. `timeout …
// join: deadline exceeded` names the budget, not the call the form was in when
// the budget ran out; the form's own output is the only account of that, and it
// is already written when the form wedges. See docs/test-runner.md § CAS asset
// capture.
//
// The fixture prints one known line and then waits far past the deadline, both
// in a single form — the runner's unit is the form, so a print in a form of its
// own would belong to a result that never timed out. `size` on the asset row is
// the UNCOMPRESSED byte length, so the exact line is pinned without
// decompressing the CAS entry.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The marker the fixture prints, and its byte length with the newline
/// `eprintln` adds.
const MARKER: &str = "before-the-hang";
const MARKER_BYTES: usize = MARKER.len() + 1;

/// Run `elle test` over a fixture that prints `MARKER` and then wedges.
/// Returns the runner's own output, the DB path, and the scratch dir.
fn run_wedged_fixture(tag: &str) -> (String, std::path::PathBuf, crate::common::ScratchDir) {
    let dir = crate::common::ScratchDir::new(tag);
    let fixture = dir.join("wedge.lisp");
    std::fs::write(
        &fixture,
        format!("(begin (eprintln \"{}\") (ev/sleep 600))\n", MARKER),
    )
    .unwrap();
    let db = dir.join("s.db");

    let out = Command::new(elle_binary())
        .args(["test"])
        .arg(&fixture)
        .args(["--timeout", "2000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert!(
        !out.status.success(),
        "a wedged form must gate non-zero; stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (combined, db, dir)
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

#[test]
fn a_timed_out_form_keeps_the_output_it_produced() {
    let (runner_output, db, _dir) = run_wedged_fixture("timeout-capture-asset");

    assert!(
        runner_output.contains("timeout"),
        "the fixture must be recorded as a timeout, got:\n{}",
        runner_output
    );

    let rows = query(
        &db,
        "SELECT a.kind AS kind, a.size AS size FROM asset a \
         JOIN result r ON r.id = a.result_id \
         WHERE r.status = 'timeout' AND a.kind = 'stderr'",
    );
    assert!(
        rows.contains(":kind \"stderr\""),
        "a timed-out form must carry its stderr, got:\n{}",
        rows
    );
    assert!(
        rows.contains(&format!(":size {}", MARKER_BYTES)),
        "the asset must hold exactly what the form printed ({} bytes), got:\n{}",
        MARKER_BYTES,
        rows
    );
}

#[test]
fn a_timed_out_form_leaves_no_capture_files_behind() {
    let (_runner_output, db, _dir) = run_wedged_fixture("timeout-capture-litter");

    // The runner redirects each (form × tier) to <dir-of-db>/scratch/*.out|.err
    // and deletes the pair once it has the bytes. An abandoned worker still
    // holds its end open, which POSIX allows: the unlink is what matters.
    let scratch = db.parent().unwrap().join("scratch");
    let left: Vec<String> = std::fs::read_dir(&scratch)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        left.is_empty(),
        "the wedged form's capture files must not outlive the run, found: {:?}",
        left
    );
}
