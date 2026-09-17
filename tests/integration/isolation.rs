// audited: 2026-09-17
// A file can have its own process: what a child's exit status becomes in the
// store, and that the run goes on after one of them dies.
//
// docs/test-runner.md
//
// The counter-factual: worker threads share one process, so the first file
// that dies on a signal takes the runner down and every result it had not
// written yet goes with it. That is why the process-global files live in
// elle_scripts.rs, where their verdicts reach no database at all. Each test
// below reads a row that could not exist under the shared-process runner.

use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Write `body` as `dir/name` and answer the path the runner is given.
fn fixture(dir: &crate::common::ScratchDir, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap_or_else(|e| panic!("write {name}: {e}"));
    path
}

/// `elle test --isolate FLAGS` over `paths`, against `db`, under `timeout_ms`.
fn isolate(db: &Path, flags: &str, timeout_ms: u64, paths: &[PathBuf]) -> std::process::Output {
    Command::new(elle_binary())
        .args(["test", "--isolate", flags])
        .args(["--timeout", &timeout_ms.to_string()])
        .arg("--db")
        .arg(db)
        .args(paths)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test --isolate")
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

/// Every result of the latest run, with the file it belongs to.
fn results(db: &Path) -> String {
    query(
        db,
        "SELECT f.file AS file, r.tier AS tier, r.status AS status, \
         r.reason AS reason, r.signal AS sig \
         FROM result r JOIN form f ON f.hash = r.form_hash \
         WHERE r.run_id = (SELECT max(id) FROM run) ORDER BY f.file",
    )
}

/// A file that kills its own process with SIGABRT.
///
/// The trap: `os/sig-raise` resolves against the signals a program may *send*,
/// and the fault set is not in it — so a fixture reaches one through `sh`,
/// where `$PPID` is this process. SIGSEGV and SIGBUS are no use here either:
/// Rust's stack-overflow guard handles both, and a *sent* one carries no
/// faulting instruction to re-raise on, so the process survives it. SIGABRT is
/// the fault signal a test can actually deliver.
const ABORTS: &str = "(elle/epoch 12)\n\
    (def k (subprocess/exec \"sh\" [\"-c\" \"kill -s ABRT $PPID\"]))\n\
    (subprocess/wait k)\n\
    (ev/sleep 10)\n";

const PASSES: &str = "(elle/epoch 12)\n(assert true \"a file that passes\")\n";

#[test]
fn an_aborting_file_is_recorded_and_the_run_completes() {
    let dir = crate::common::ScratchDir::new("isolate-abort");
    let db = dir.join("s.db");
    let aborts = fixture(&dir, "aborts.lisp", ABORTS);
    let passes = fixture(&dir, "passes.lisp", PASSES);

    let out = isolate(&db, "", 30000, &[aborts, passes]);
    assert!(
        !out.status.success(),
        "a fail must gate the run non-zero; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The run reached its end: the runner outlived the child that died.
    let run = query(
        &db,
        "SELECT (finished_at IS NOT NULL) AS done, tiers AS tiers \
         FROM run WHERE id = (SELECT max(id) FROM run)",
    );
    assert!(
        run.contains(":done 1"),
        "the run must complete after a child dies, got:\n{run}"
    );
    assert!(
        run.contains(":tiers \"process\""),
        "an isolated run records the process tier, got:\n{run}"
    );

    let rows = results(&db);
    assert!(
        rows.contains("aborts.lisp") && rows.contains(":status \"fail\""),
        "the aborting file must be one fail row, got:\n{rows}"
    );
    assert!(
        rows.contains("SIGABRT"),
        "the fail must name the signal that killed the child, got:\n{rows}"
    );
    assert!(
        rows.contains(":sig \":sigabrt\""),
        "and record it as the result's signal, got:\n{rows}"
    );
    assert!(
        rows.contains("passes.lisp") && rows.contains(":status \"pass\""),
        "the run must go on to the next path, got:\n{rows}"
    );
}

#[test]
fn a_nonzero_exit_is_a_fail_naming_the_code() {
    let dir = crate::common::ScratchDir::new("isolate-exit");
    let db = dir.join("s.db");
    let path = fixture(
        &dir,
        "exits.lisp",
        "(elle/epoch 12)\n(os/exit 3)\n",
    );

    let out = isolate(&db, "", 30000, &[path]);
    assert!(!out.status.success(), "a non-zero child gates the run");

    let rows = results(&db);
    assert!(
        rows.contains(":status \"fail\"") && rows.contains("exit 3"),
        "a child's exit code is the reason, got:\n{rows}"
    );
}

#[test]
fn a_childs_output_becomes_an_asset() {
    let dir = crate::common::ScratchDir::new("isolate-assets");
    let db = dir.join("s.db");
    let path = fixture(
        &dir,
        "talks.lisp",
        "(elle/epoch 12)\n(println \"on stdout\")\n(eprintln \"on stderr\")\n",
    );

    let out = isolate(&db, "", 30000, &[path]);
    assert!(
        out.status.success(),
        "a chatty child that exits 0 still passes; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let assets = query(
        &db,
        "SELECT a.kind AS kind, (a.size > 0) AS sized FROM asset a \
         JOIN result r ON r.id = a.result_id \
         WHERE r.run_id = (SELECT max(id) FROM run) ORDER BY a.kind",
    );
    assert!(
        assets.contains(":kind \"stdout\"") && assets.contains(":kind \"stderr\""),
        "both of a child's streams become assets, got:\n{assets}"
    );
    assert!(
        !assets.contains(":sized 0"),
        "an asset that captured nothing would not be one, got:\n{assets}"
    );
}

#[test]
fn a_child_over_its_budget_is_a_timeout() {
    let dir = crate::common::ScratchDir::new("isolate-budget");
    let db = dir.join("s.db");
    let path = fixture(
        &dir,
        "slow.lisp",
        "(elle/epoch 12)\n(println \"starting\")\n(ev/sleep 60)\n",
    );

    let out = isolate(&db, "", 1500, &[path]);
    assert!(!out.status.success(), "a timeout gates the run non-zero");

    let rows = results(&db);
    assert!(
        rows.contains(":status \"timeout\""),
        "a child over its budget is a timeout, not a fail, got:\n{rows}"
    );

    // What it printed before the kill is the only account of where it was.
    let assets = query(
        &db,
        "SELECT a.kind AS kind FROM asset a JOIN result r ON r.id = a.result_id \
         WHERE r.run_id = (SELECT max(id) FROM run)",
    );
    assert!(
        assets.contains(":kind \"stdout\""),
        "a killed child keeps what it printed, got:\n{assets}"
    );
}

/// A gated child exits 0, so its exit status alone reads as a vacuous pass —
/// the coverage-hiding failure the loud gate exists to prevent. The binary
/// prints `SKIP (gated):` on that path and the runner reads it.
#[test]
fn a_gated_child_is_a_skip_carrying_its_reason() {
    let dir = crate::common::ScratchDir::new("isolate-gated");
    let db = dir.join("s.db");
    let path = fixture(
        &dir,
        "gated.lisp",
        "(elle/epoch 12)\n(error (struct :error :gated :reason \"no widget here\"))\n",
    );

    let out = isolate(&db, "", 30000, &[path]);
    assert!(
        out.status.success(),
        "a skip is not a failure, so the run gates green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let rows = results(&db);
    assert!(
        rows.contains(":status \"skip\""),
        "a gated child is a skip, never a pass, got:\n{rows}"
    );
    assert!(
        rows.contains("no widget here"),
        "and the skip carries the reason the gate gave, got:\n{rows}"
    );
}

/// The flags reach the child. Without this the mode the path exists for —
/// `--trace=guardfree`, `--no-uring` — would be dropped silently and every
/// file would run under the default configuration, green and meaningless.
#[test]
fn the_flags_reach_the_child() {
    let dir = crate::common::ScratchDir::new("isolate-flags");
    let db = dir.join("s.db");
    let path = fixture(
        &dir,
        "policy.lisp",
        "(elle/epoch 12)\n\
         (assert (= (vm/config :jit) :off) \"the child must run under the flags it was given\")\n",
    );

    let out = isolate(&db, "--jit=off", 30000, &[path]);
    assert!(
        out.status.success(),
        "the child must see --jit=off; stderr:\n{}\nresults:\n{}",
        String::from_utf8_lossy(&out.stderr),
        results(&db)
    );
}
