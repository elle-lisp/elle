// audited: 2026-09-22
// A run recorded in another store joins local history — once, with the code
// state it ran against and the bytes its assets name.
//
// docs/test-store.md
//
// The counter-factual: with no merge a downloaded run is a database you point
// `--db` at, so it answers alone and never joins a local query. With no run
// key a second import of one artifact appends every run again, and the history
// then reads as twice the runs rather than as one run seen twice.

use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A store's session DB, with its CAS directory beside it, as `--db` lays one
/// out. Each test gives a store its own directory so the two never share a CAS
/// and a copied file is provably copied.
fn store(dir: &Path, name: &str) -> PathBuf {
    let home = dir.join(name);
    std::fs::create_dir_all(&home).expect("create the store directory");
    home.join("elle-tests.db")
}

fn cas(db: &Path) -> PathBuf {
    db.parent().expect("a store has a directory").join("cas")
}

/// A one-form file that prints, so the run it records carries a `stdout`
/// asset and the import has bytes to copy.
///
/// The printed text names the file. A form is identified by the hash of its
/// syntax, so two fixtures with one body would be one form in both stores, and
/// a merge that dropped the foreign row would still look right.
fn fixture(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("(println \"printed by {name}\")\n")).expect("write the fixture");
    path
}

/// Record one run of `fixture` into `db`, from the repository (so the run row
/// carries a commit, a worktree and a tree hash to compare after the import).
fn record(db: &Path, fixture: &Path) -> std::process::Output {
    Command::new(elle_binary())
        .args(["test"])
        .arg(fixture)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(db)
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test")
}

/// Merge `src` into `db`.
fn import(db: &Path, src: &Path) -> std::process::Output {
    Command::new(elle_binary())
        .args(["test", "--import"])
        .arg(src)
        .arg("--db")
        .arg(db)
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test --import")
}

/// The rendered rows of `sql` against `db`.
fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(elle_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query a session DB");
    assert!(
        out.status.success(),
        "the query failed: {sql}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// One integer a query rendered under the alias `c`.
fn scalar(db: &Path, sql: &str) -> i64 {
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

fn rows_in(db: &Path, table: &str) -> i64 {
    scalar(db, &format!("SELECT count(*) AS c FROM {table}"))
}

/// The whole store, as the counts an import has to reproduce.
fn census(db: &Path) -> Vec<(&'static str, i64)> {
    ["run", "form", "result", "asset", "measurement", "gauge", "changed_file"]
        .iter()
        .map(|t| (*t, rows_in(db, t)))
        .collect()
}

#[test]
fn an_imported_run_joins_local_history() {
    let dir = crate::common::ScratchDir::new("import-joins");
    let far = store(dir.path(), "far");
    let near = store(dir.path(), "near");
    let out = record(&far, &fixture(dir.path(), "printer.lisp"));
    assert!(
        out.status.success(),
        "the recorded run should gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let done = import(&near, &far);
    assert!(
        done.status.success(),
        "the import should succeed; stderr:\n{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let report = String::from_utf8_lossy(&done.stderr);
    assert!(
        report.contains("imported 1 run"),
        "the import must say what it took, got:\n{report}"
    );

    // The same identity, read from both stores. A run whose commit, worktree,
    // host and argv did not survive the merge names no code and no machine,
    // which is the whole reason the columns exist.
    let identity = "SELECT git_commit AS sha, worktree AS worktree, host AS host, \
                    argv AS argv, tiers AS tiers, n_selected AS sel, \
                    (finished_at IS NOT NULL) AS done FROM run";
    assert_eq!(
        query(&near, identity),
        query(&far, identity),
        "an imported run must carry the identity the recorded run had"
    );

    for (table, n) in census(&far) {
        assert_eq!(
            rows_in(&near, table),
            n,
            "the import must bring every {table} row"
        );
    }

    // The join a local result answers is the one an imported result has to
    // answer: a form, by the hash of its syntax.
    let joined = query(
        &near,
        "SELECT f.file AS file, r.tier AS tier, r.status AS status \
         FROM result r JOIN form f ON f.hash = r.form_hash ORDER BY r.tier",
    );
    assert!(
        joined.contains("printer.lisp"),
        "an imported result must join to its form, got:\n{joined}"
    );
}

#[test]
fn importing_the_same_store_twice_changes_nothing() {
    let dir = crate::common::ScratchDir::new("import-twice");
    let far = store(dir.path(), "far");
    let near = store(dir.path(), "near");
    record(&far, &fixture(dir.path(), "printer.lisp"));

    import(&near, &far);
    let once = census(&near);
    let again = import(&near, &far);
    assert!(
        again.status.success(),
        "a repeated import should succeed; stderr:\n{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let report = String::from_utf8_lossy(&again.stderr);
    assert!(
        report.contains("already present"),
        "a repeated import must say the run was already here, got:\n{report}"
    );
    assert_eq!(
        census(&near),
        once,
        "a run already imported must not be appended a second time"
    );
}

#[test]
fn importing_a_store_into_itself_changes_nothing() {
    let dir = crate::common::ScratchDir::new("import-self");
    let near = store(dir.path(), "near");
    record(&near, &fixture(dir.path(), "printer.lisp"));
    let before = census(&near);

    let done = import(&near, &near);
    assert!(
        done.status.success(),
        "importing a store into itself should succeed; stderr:\n{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        census(&near),
        before,
        "every run in the source is already in the destination"
    );
}

#[test]
fn an_imported_asset_brings_its_bytes() {
    let dir = crate::common::ScratchDir::new("import-cas");
    let far = store(dir.path(), "far");
    let near = store(dir.path(), "near");
    record(&far, &fixture(dir.path(), "printer.lisp"));

    let addresses = query(&far, "SELECT hash AS hash FROM asset");
    assert!(
        !addresses.trim().is_empty(),
        "the fixture prints, so the recorded run must hold an asset"
    );
    import(&near, &far);

    let mut seen = 0;
    for entry in std::fs::read_dir(cas(&far)).expect("read the source CAS") {
        let entry = entry.expect("a CAS entry");
        let there = std::fs::read(entry.path()).expect("read the source bytes");
        let here = cas(&near).join(entry.file_name());
        assert!(
            here.exists(),
            "the import must copy {:?} into the local CAS",
            entry.file_name()
        );
        assert_eq!(
            std::fs::read(&here).expect("read the copied bytes"),
            there,
            "a CAS file is its content, so the copy must be byte-identical"
        );
        seen += 1;
    }
    assert!(seen > 0, "the source CAS held nothing, so nothing was proven");
}

/// An asset and a measurement name a result by its row id, and the ids of two
/// stores mean nothing to each other. A merge that kept the foreign ids would
/// point every imported asset at whichever local result happened to hold that
/// number.
#[test]
fn a_result_id_is_remapped_as_the_row_lands() {
    let dir = crate::common::ScratchDir::new("import-remap");
    let far = store(dir.path(), "far");
    let near = store(dir.path(), "near");
    record(&far, &fixture(dir.path(), "printer.lisp"));

    // The local store gets a run of its own first, so its result ids are
    // already taken when the foreign rows arrive.
    record(&near, &fixture(dir.path(), "local.lisp"));

    // A dashboard's verdict and a changed file, seeded: no run records either
    // yet, and both hang off a run the merge has to carry.
    query(
        &far,
        "INSERT INTO measurement (run_id, result_id, subject, axis, value, unit, verdict) \
         SELECT run_id, id, 'a-probe', 'regions', 1.5, 'regions/op', 'closed' \
         FROM result ORDER BY id LIMIT 1",
    );
    query(
        &far,
        "INSERT INTO changed_file (run_id, path, status, blob_hash) \
         SELECT id, 'src/vm.rs', 'M', 'cafe1234' FROM run",
    );

    import(&near, &far);

    // The imported measurement must join to the imported result — the one for
    // the form the foreign store recorded, not a local row of the same number.
    let joined = query(
        &near,
        "SELECT f.file AS file, r.tier AS tier FROM measurement m \
         JOIN result r ON r.id = m.result_id JOIN form f ON f.hash = r.form_hash \
         WHERE m.subject = 'a-probe'",
    );
    assert!(
        joined.contains("printer.lisp"),
        "the imported measurement must point at the imported result, got:\n{joined}"
    );

    let assets = scalar(
        &near,
        "SELECT count(*) AS c FROM asset a JOIN result r ON r.id = a.result_id",
    );
    assert_eq!(
        assets,
        rows_in(&near, "asset"),
        "every imported asset must point at a result that exists"
    );

    let changed = query(
        &near,
        "SELECT cf.path AS path, r.host AS host FROM changed_file cf \
         JOIN run r ON r.id = cf.run_id",
    );
    assert!(
        changed.contains("src/vm.rs"),
        "a changed file must follow its run, got:\n{changed}"
    );
}

/// A run recorded before the key existed carries none, and two imports of one
/// such store must still leave one copy of it.
#[test]
fn a_run_with_no_key_imports_once() {
    let dir = crate::common::ScratchDir::new("import-keyless");
    let far = store(dir.path(), "far");
    let near = store(dir.path(), "near");
    record(&far, &fixture(dir.path(), "printer.lisp"));
    query(&far, "UPDATE run SET run_key = NULL");

    import(&near, &far);
    import(&near, &far);
    assert_eq!(
        rows_in(&near, "run"),
        1,
        "a keyless run is identified by the row it is, so it imports once"
    );
}

/// A path that names no store is a mistake, not an empty import: answering
/// zero would report success having merged nothing.
#[test]
fn an_import_of_a_store_that_is_not_there_fails() {
    let dir = crate::common::ScratchDir::new("import-missing");
    let near = store(dir.path(), "near");
    let absent = dir.path().join("nowhere").join("elle-tests.db");

    let out = import(&near, &absent);
    assert!(
        !out.status.success(),
        "an import of a missing store must not report success"
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains(absent.to_str().expect("utf-8 path")),
        "the message must name the path it could not read, got:\n{said}"
    );
    assert_eq!(
        rows_in(&near, "run"),
        0,
        "a failed import must leave no run behind"
    );
}
