// audited: 2026-11-21
// A form row says what the compiler found in the form: the signals it may
// emit, the capabilities among them, and the bindings it calls.
//
// docs/test-store.md
//
// The counter-factual: the three columns existed and nothing ever wrote them,
// so every form read the same NULL. Selection by capability had no data,
// nothing could ask which forms are pure, and a cache had no way to tell a
// replayable form from one that opens a file.
//
// The trap: an empty column and a NULL are different claims. Empty says the
// compiler proved the form reaches no capability; NULL says the analysis did
// not answer. A test that accepted either would pass against a runner that
// analyzed nothing at all.

use crate::common::query;
use std::path::Path;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A form that writes a file and reads it back, through a helper it defines
/// itself. Its capabilities are `fs` and `io`, its callees are primitives, and
/// `write-it` is its own — so the row can be read for what belongs in each
/// column and what does not.
const RICH: &str = "(defn write-it [p]\n\
                    \x20 (let [out (port/open p :write)]\n\
                    \x20   (port/write out \"hi\")\n\
                    \x20   (port/close out)))\n\
                    (write-it \"@\")\n\
                    (assert (= (slurp \"@\") \"hi\") \"the file round-trips\")\n";

/// One form, reaching nothing. The compiler can prove this one pure, which is
/// what an empty capability set means.
const PURE: &str = "(assert (= 1 1) \"arithmetic holds\")\n";

/// A file that runs green and does not analyze as the one function the profile
/// is read from. A file registers every signal it declares before its first
/// form, and a function body registers one at its declaration, so the body's
/// `emit` names an unknown signal. The analysis refuses, and the row must say
/// so rather than guess.
const REFUSED: &str = "(defn raise-it [] (emit :form-profile-late 1))\n\
                       (signal :form-profile-late)\n\
                       (assert (fn? raise-it) \"the file runs\")\n";

/// One form that reads the pid, which is ambient process state and nothing
/// else. Its capability set is `ambient` alone.
const AMBIENT: &str = "(assert (> (sys/pid) 0) \"the pid is positive\")\n";

/// A call through a table, which the compiler cannot resolve to a function.
/// The analysis answers every capability bit for it, so the row shows the
/// whole set the runner records.
const UNRESOLVED: &str = "(def table @{:answer (fn [] 42)})\n\
                          (assert (= ((get table :answer)) 42) \"the table's function answers\")\n";

/// Run the fixtures in one `elle test`, and answer with the session DB and the
/// scratch directory that owns it.
fn run_fixtures(tag: &str) -> (std::path::PathBuf, crate::common::ScratchDir) {
    let dir = crate::common::ScratchDir::new(tag);
    let db = dir.join("s.db");
    let written = dir.join("written.txt");

    let mut cmd = Command::new(elle_binary());
    cmd.args(["test"]);
    for (name, body) in [
        (
            "rich.lisp",
            RICH.replace('@', written.to_str().expect("utf-8 path")),
        ),
        ("pure.lisp", PURE.to_string()),
        ("refused.lisp", REFUSED.to_string()),
        ("ambient.lisp", AMBIENT.to_string()),
        ("unresolved.lisp", UNRESOLVED.to_string()),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write fixture");
        cmd.arg(&path);
    }
    let out = cmd
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(&db)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    assert!(
        out.status.success(),
        "every fixture passes, so the run gates green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (db, dir)
}

/// Ask whether one space-separated column of `file`'s form row holds `word`.
/// A column is a list of names, so membership is the question — never whether
/// one name is a substring of another.
fn holds(db: &Path, file: &str, column: &str, word: &str) -> String {
    query(
        db,
        &format!(
            "SELECT (instr(' ' || {column} || ' ', ' {word} ') > 0) AS held \
             FROM form WHERE file LIKE '%/{file}'"
        ),
    )
}

#[test]
fn a_form_that_writes_a_file_records_the_capabilities_it_reaches() {
    let (db, _dir) = run_fixtures("form-profile-caps");

    for (column, word) in [
        ("caps", "io"),
        ("caps", "fs"),
        ("signal", "error"),
        ("signal", "io"),
    ] {
        let rows = holds(&db, "rich.lisp", column, word);
        assert!(
            rows.contains(":held 1"),
            "a form that opens a port records {word} in {column}, got:\n{rows}"
        );
    }

    let rows = holds(&db, "rich.lisp", "caps", "error");
    assert!(
        rows.contains(":held 0"),
        "error is a signal, not a capability, got:\n{rows}"
    );
}

/// Every capability bit the runtime builds in, but `error`, `yield` and `halt`,
/// sorted and space-separated as the `caps` column writes them. It reads the
/// registry rather than a list, so a bit the registry gains joins it here.
fn built_in_capabilities() -> String {
    let registry = elle::signals::registry::SignalRegistry::with_builtins();
    let mut names: Vec<&str> = registry
        .entries()
        .iter()
        .filter(|e| elle::signals::CAP_MASK.has_bit(e.bit_position))
        .map(|e| e.name.as_str())
        .filter(|n| !["error", "yield", "halt"].contains(n))
        .collect();
    names.sort_unstable();
    names.join(" ")
}

/// The counter-factual stopped the runner's list at `os-signal`, so a form
/// that read the pid recorded an empty set: a purity the compiler never proved.
#[test]
fn a_form_that_reads_the_pid_records_ambient() {
    let (db, _dir) = run_fixtures("form-profile-ambient");

    let rows = query(
        &db,
        "SELECT (caps = 'ambient') AS ambient FROM form WHERE file LIKE '%/ambient.lisp'",
    );
    assert!(
        rows.contains(":ambient 1"),
        "a form whose one capability is the pid records ambient alone, got:\n{rows}"
    );
}

/// The census: a call the compiler cannot resolve reaches every capability, so
/// its row names each one. The counter-factual recorded seven names, and a
/// bit added to the registry after them never reached the column.
#[test]
fn an_unresolved_call_records_every_built_in_capability() {
    let (db, _dir) = run_fixtures("form-profile-census");

    let expected = built_in_capabilities();
    let rows = query(
        &db,
        &format!(
            "SELECT (caps = '{expected}') AS census, caps FROM form \
             WHERE file LIKE '%/unresolved.lisp'"
        ),
    );
    assert!(
        rows.contains(":census 1"),
        "the row names every built-in capability, {expected}, got:\n{rows}"
    );
}

#[test]
fn a_form_touches_what_it_calls_and_not_what_it_defines() {
    let (db, _dir) = run_fixtures("form-profile-touches");

    for word in ["port/open", "port/write", "slurp"] {
        let rows = holds(&db, "rich.lisp", "touches", word);
        assert!(
            rows.contains(":held 1"),
            "the form calls {word}, so its row must say so, got:\n{rows}"
        );
    }

    let rows = holds(&db, "rich.lisp", "touches", "write-it");
    assert!(
        rows.contains(":held 0"),
        "the form defines write-it, so it does not touch it, got:\n{rows}"
    );
}

#[test]
fn a_pure_form_records_an_empty_capability_set() {
    let (db, _dir) = run_fixtures("form-profile-pure");

    let rows = query(
        &db,
        "SELECT (caps = '') AS pure, (caps IS NULL) AS unanalyzed, \
         (signal = 'error') AS signal FROM form WHERE file LIKE '%/pure.lisp'",
    );
    assert!(
        rows.contains(":pure 1") && rows.contains(":unanalyzed 0"),
        "a form reaching nothing records an empty capability set, not NULL, got:\n{rows}"
    );
    assert!(
        rows.contains(":signal 1"),
        "an assert can raise, so the form's signal is error, got:\n{rows}"
    );
}

#[test]
fn a_file_the_analysis_refuses_records_null_and_still_runs() {
    let (db, _dir) = run_fixtures("form-profile-null");

    // Both files ran in this one run, so the contrast is the assertion: the
    // NULL is this file's answer, not a runner that analyzed nothing.
    let rows = query(
        &db,
        "SELECT (caps IS NULL) AS nocaps, (touches IS NULL) AS notouches, \
         (signal IS NULL) AS nosignal FROM form WHERE file LIKE '%/refused.lisp'",
    );
    assert!(
        rows.contains(":nocaps 1") && rows.contains(":notouches 1") && rows.contains(":nosignal 1"),
        "an analysis that does not answer records NULL in all three, got:\n{rows}"
    );
    let profiled = query(
        &db,
        "SELECT count(*) AS n FROM form WHERE file LIKE '%/pure.lisp' AND caps IS NOT NULL",
    );
    assert!(
        profiled.contains(":n 1"),
        "the same run profiled the file that does analyze, got:\n{profiled}"
    );

    // The run gated green in run_fixtures, which is the other half: a profile
    // the compiler will not produce costs the columns and nothing else.
    let results = query(
        &db,
        "SELECT count(*) AS passed FROM result r JOIN form f ON f.hash = r.form_hash \
         WHERE f.file LIKE '%/refused.lisp' AND r.status = 'pass'",
    );
    assert!(
        !results.contains(":passed 0"),
        "the file still runs and passes, got:\n{results}"
    );
}
