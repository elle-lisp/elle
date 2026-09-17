// audited: 2026-09-17
// A `run` row names the code it ran against — commit, worktree, host, build —
// so a result belongs to something and a warning can say whose run it warns
// about.
//
// docs/test-store.md
//
// The counter-factual: without these columns every row reads the same whatever
// commit produced it, regression archaeology has nothing to group by, and the
// killed-run warning fires on a sibling checkout's live run with no way to
// tell the reader that is what happened.

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The trimmed stdout of a command run in `cwd`, or the empty string when it
/// fails. The tests compare the runner's answer against the same tools the
/// runner reads, so a box that spells its hostname differently still agrees
/// with itself.
fn shell(cwd: &Path, program: &str, args: &[&str]) -> String {
    let out = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("run {program}: {e}"));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run `elle test` over a trivial passing form, from `cwd`, against `db`.
fn run_in(cwd: &Path, fixture: &Path, db: &Path) -> std::process::Output {
    std::fs::write(fixture, "(assert true \"ok\")\n").expect("write fixture");
    Command::new(elle_binary())
        .args(["test"])
        .arg(fixture)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(db)
        .current_dir(cwd)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
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
fn a_run_row_carries_the_commit_and_the_host() {
    let dir = crate::common::ScratchDir::new("run-identity-commit");
    let db = dir.join("s.db");
    let root = repo_root();
    let out = run_in(&root, &dir.join("pass.lisp"), &db);
    assert!(
        out.status.success(),
        "the run should gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // `commit` is an SQLite keyword, so the commit column answers to `sha`
    // here. Every alias below is a column name the row is read back by.
    let rows = query(
        &db,
        "SELECT git_commit AS sha, host AS host, worktree AS worktree, \
         elle_version AS version, build_profile AS profile, \
         (length(tree_hash) > 0) AS hastree FROM run WHERE id = (SELECT max(id) FROM run)",
    );

    let head = shell(&root, "git", &["rev-parse", "HEAD"]);
    let worktree = shell(&root, "git", &["rev-parse", "--show-toplevel"]);
    let host = shell(&root, "uname", &["-n"]);
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    for (what, expected) in [
        ("sha", head),
        ("host", host),
        ("worktree", worktree),
        ("version", env!("CARGO_PKG_VERSION").to_string()),
        ("profile", profile.to_string()),
    ] {
        assert!(
            rows.contains(&format!(":{what} \"{expected}\"")),
            "the run row must record {what} as {expected:?}, got:\n{rows}"
        );
    }
    assert!(
        rows.contains(":hastree 1"),
        "the run row must record a tree hash, got:\n{rows}"
    );
}

/// Outside a repository there is no code state to record. NULL says that; a
/// made-up value would say something false.
#[test]
fn a_run_outside_a_repository_records_no_commit() {
    let dir = crate::common::ScratchDir::new("run-identity-norepo");
    let db = dir.join("s.db");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("create a directory outside any checkout");
    let out = run_in(&elsewhere, &dir.join("pass.lisp"), &db);
    assert!(
        out.status.success(),
        "a run outside a repository still gates on its results; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let rows = query(
        &db,
        "SELECT (git_commit IS NULL) AS nocommit, (worktree IS NULL) AS noworktree, \
         (length(host) > 0) AS hashost FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        rows.contains(":nocommit 1") && rows.contains(":noworktree 1"),
        "no repository means no commit and no worktree, got:\n{rows}"
    );
    assert!(
        rows.contains(":hashost 1"),
        "the host is a fact about the machine, not the checkout, got:\n{rows}"
    );
}

/// A tally in a log says nothing until it says which code it describes.
#[test]
fn the_summary_names_the_commit() {
    let dir = crate::common::ScratchDir::new("run-identity-summary");
    let db = dir.join("s.db");
    let root = repo_root();
    run_in(&root, &dir.join("pass.lisp"), &db);

    let out = Command::new(elle_binary())
        .args(["test", "--summary"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("summary");
    let summary = String::from_utf8_lossy(&out.stderr);
    let head = shell(&root, "git", &["rev-parse", "HEAD"]);
    let short = &head[..7];
    assert!(
        summary.contains(short),
        "--summary must name the run's commit ({short}), got:\n{summary}"
    );
}

/// One session DB serves every checkout on the box, so the warning has to say
/// whose run it found unfinished. A sibling worktree running its own corpus
/// leaves exactly the same row a kill does.
#[test]
fn the_kill_warning_names_the_worktree() {
    let dir = crate::common::ScratchDir::new("run-identity-worktree");
    let db = dir.join("s.db");
    let root = repo_root();
    let killer = dir.join("kills.lisp");
    std::fs::write(&killer, "(os/sig-raise :sigkill)\n").expect("write fixture");

    let killed = Command::new(elle_binary())
        .args(["test"])
        .arg(&killer)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .current_dir(&root)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert_eq!(
        killed.status.signal(),
        Some(9),
        "the fixture must SIGKILL the runner; stderr:\n{}",
        String::from_utf8_lossy(&killed.stderr)
    );

    let next = run_in(&root, &dir.join("pass.lisp"), &db);
    let warning = String::from_utf8_lossy(&next.stderr);
    let worktree = shell(&root, "git", &["rev-parse", "--show-toplevel"]);
    assert!(
        warning.contains("was killed"),
        "the next run must warn about its killed predecessor, got:\n{warning}"
    );
    assert!(
        warning.contains(&worktree),
        "the warning must name the worktree it warns about ({worktree}), got:\n{warning}"
    );
}
