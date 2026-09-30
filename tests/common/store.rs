// audited: 2026-09-30
//! Reading an `elle test` session store through the binary's own `--query`.
//!
//! docs/test-store.md

use std::path::Path;
use std::process::Command;

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
