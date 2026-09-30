// audited: 2026-09-30
// A path the runner cannot read, and a source that does not parse, each become
// one file-level fail row, and the run goes on to the next path.
//
// docs/test-runner.md
//
// The counter-factual: the runner reads a file before it compiles it, and a
// read that raises outside `protect` ends the whole run. The run row then says
// DID NOT COMPLETE, the paths after the bad one never run, and the error names
// the runner's own source line rather than the file at fault.

use crate::common::query;
use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A file that passes, run after the bad one to show the run went on.
const PASSES: &str = "(assert true \"the next path runs\")\n";

/// Write `body` as `dir/name` and answer its path.
fn fixture(dir: &crate::common::ScratchDir, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap_or_else(|e| panic!("write {name}: {e}"));
    path
}

/// `elle test ARGS` against `db`.
fn run(db: &Path, args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(elle_binary())
        .arg("test")
        .args(args)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
}

/// Every result of the latest run with the file its form names, and whether
/// the run reached its end.
fn outcome(db: &Path) -> (String, String) {
    let rows = query(
        db,
        "SELECT f.file AS file, f.form_index AS idx, r.tier AS tier, \
         r.status AS status, r.reason AS reason \
         FROM result r JOIN form f ON f.hash = r.form_hash \
         WHERE r.run_id = (SELECT max(id) FROM run) ORDER BY r.id",
    );
    let run = query(
        db,
        "SELECT (finished_at IS NOT NULL) AS done, n_fail AS fails \
         FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    (rows, run)
}

/// Assert the shape every case shares: the run finished with one fail, the
/// bad source is one file-level `vm` row whose reason carries `said`, and the
/// path after it passed.
fn assert_one_file_level_fail(out: &std::process::Output, db: &Path, bad: &str, said: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a file-level failure gates the run non-zero; stderr:\n{stderr}"
    );
    let (rows, run) = outcome(db);
    assert!(
        run.contains(":done 1"),
        "the run must reach its end; run row:\n{run}\nstderr:\n{stderr}"
    );
    assert!(
        run.contains(":fails 1"),
        "the bad source is one fail, got:\n{run}\nresults:\n{rows}"
    );
    let bad_row = rows
        .lines()
        .find(|l| l.contains(bad))
        .unwrap_or_else(|| panic!("no row names {bad}, got:\n{rows}"));
    for want in [":idx -1", ":tier \"vm\"", ":status \"fail\""] {
        assert!(
            bad_row.contains(want),
            "the bad source is one file-level vm fail ({want}), got:\n{bad_row}"
        );
    }
    assert!(
        bad_row.contains(said),
        "the reason must carry the error ({said}), got:\n{bad_row}"
    );
    assert!(
        rows.lines()
            .any(|l| l.contains("passes.lisp") && l.contains(":status \"pass\"")),
        "the path after the bad one must run and pass, got:\n{rows}"
    );
}

#[test]
fn a_path_that_cannot_be_read_is_one_file_level_failure() {
    let dir = crate::common::ScratchDir::new("file-error-missing");
    let db = dir.join("s.db");
    let missing = dir.join("missing.lisp");
    let passes = fixture(&dir, "passes.lisp", PASSES);

    let out = run(&db, &[missing.as_os_str(), passes.as_os_str()]);
    assert_one_file_level_fail(&out, &db, "missing.lisp", "No such file");
}

#[test]
fn a_file_that_does_not_parse_is_one_file_level_failure() {
    let dir = crate::common::ScratchDir::new("file-error-unparsed");
    let db = dir.join("s.db");
    let unclosed = fixture(&dir, "unclosed.lisp", "(assert true \"unclosed\"\n");
    let passes = fixture(&dir, "passes.lisp", PASSES);

    let out = run(&db, &[unclosed.as_os_str(), passes.as_os_str()]);
    assert_one_file_level_fail(&out, &db, "unclosed.lisp", "unterminated list");
}

/// An `-e` form has no path, so the run's order is the paths, then the forms.
/// The passing path runs first here, and the bad form is the run's last word.
#[test]
fn an_eval_that_does_not_parse_is_one_file_level_failure() {
    let dir = crate::common::ScratchDir::new("file-error-eval");
    let db = dir.join("s.db");
    let passes = fixture(&dir, "passes.lisp", PASSES);

    let out = run(
        &db,
        &[passes.as_os_str(), "-e".as_ref(), "(+ 1".as_ref()],
    );
    assert_one_file_level_fail(&out, &db, "<eval>", "unterminated list");
}
