// audited: 2026-11-22
//! Running `elle test` into a session store, and reading the store back through `--query`.
//!
//! docs/test-store.md

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output};

/// The rows `sql` answers from the store at `db`, as `--query` prints them.
///
/// A query that fails panics with its stderr. An empty answer from a query
/// that failed reads the same as one that matched nothing, so an assertion
/// on it would pass for the wrong reason.
#[allow(dead_code)]
pub fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    assert!(
        out.status.success(),
        "the query failed: {sql}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run `elle test ARGS` into the store at `db`, and answer what it left. The
/// caller decides which exit status it wants. `RUST_MIN_STACK` is removed, as
/// every runner test here starts the runner.
#[allow(dead_code)]
pub fn elle_test<I, S>(db: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("test")
        .args(args)
        .arg("--db")
        .arg(db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
}

/// The one integer `sql` answers under the alias `c`.
#[allow(dead_code)]
pub fn scalar(db: &Path, sql: &str) -> i64 {
    let rows = query(db, sql);
    let at = rows
        .find(":c ")
        .unwrap_or_else(|| panic!("no `c` column in:\n{rows}"));
    let digits: String = rows[at + 3..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits
        .parse()
        .unwrap_or_else(|e| panic!("`c` is not a number ({e}) in:\n{rows}"))
}
