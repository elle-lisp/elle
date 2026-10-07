// audited: 2026-10-06
// A file's charge: what its second run in-process cost the runner's heap, judged as the producer `elle test`.
// docs/test-gauges.md
// docs/ratchet.md
//
// The counter-factual: a window over a file's first run read 23 objects more
// for the first file of a batch, 2 objects and a region less after a file
// that filled the same cache, and a different page count on a fresh store
// than on a warm one. A pin over that reading fails whenever a batch is dealt
// differently.

mod common;

use common::ratchet::{own, query, rig_binary};
use common::{stderr, stdout, Scratch};
use std::path::Path;
use std::process::{Command, Output};

/// The repository the rig was built in, whose language suite supplies files
/// that share a cache entry.
fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rig sits in the repository")
}

/// `elle-rig test ARGS` from `dir`, recording into `db`.
fn run(dir: &Path, db: &Path, args: &[&str]) -> Output {
    Command::new(rig_binary())
        .arg("test")
        .args(args)
        .args(["--timeout", "60000"])
        .arg("--db")
        .arg(db)
        .current_dir(dir)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle-rig test")
}

/// The latest run's readings of `subject`, as `axis=value` in axis order.
fn charge_of(db: &Path, subject: &str) -> String {
    query(
        db,
        &format!(
            "SELECT axis, value FROM measurement \
             WHERE run_id = (SELECT max(id) FROM run) AND subject = '{subject}' \
             AND value IS NOT NULL ORDER BY axis"
        ),
    )
}

/// The latest run's verdict for `subject` on `axis`.
fn verdict(db: &Path, subject: &str, axis: &str) -> String {
    query(
        db,
        &format!(
            "SELECT verdict FROM measurement \
             WHERE run_id = (SELECT max(id) FROM run) AND subject = '{subject}' \
             AND axis = '{axis}'"
        ),
    )
}

/// Every row the latest run recorded about `subject`: read, judged or missing.
fn rows_of(db: &Path, subject: &str) -> String {
    query(
        db,
        &format!(
            "SELECT axis, value, verdict FROM measurement \
             WHERE run_id = (SELECT max(id) FROM run) AND subject = '{subject}'"
        ),
    )
}

/// The distinct statuses of the latest run's results, in order.
fn statuses(db: &Path) -> String {
    query(
        db,
        "SELECT DISTINCT status FROM result \
         WHERE run_id = (SELECT max(id) FROM run) ORDER BY status",
    )
}

/// A scratch working directory holding `f.lisp`, `g.lisp` and `h.lisp`,
/// one-form files, and the runner's ledger files under `tests/ledger`, each
/// `(name, rows)`.
fn bench(tag: &str, ledgers: &[(&str, Vec<String>)]) -> Scratch {
    let dir = Scratch::new(&format!("charge-{tag}"));
    dir.write("f.lisp", "(assert (= (+ 1 1) 2) \"one and one\")\n");
    dir.write("g.lisp", "(assert (= (+ 2 2) 4) \"two and two\")\n");
    dir.write("h.lisp", "(assert (= (+ 3 3) 6) \"three and three\")\n");
    std::fs::create_dir_all(dir.path().join("tests/ledger")).expect("create the ledger dir");
    for (name, rows) in ledgers {
        dir.write(
            &format!("tests/ledger/{name}"),
            &format!(
                "(elle/epoch 14)\n(producer \"elle test\")\n{}\n",
                rows.join("\n")
            ),
        );
    }
    dir
}

#[test]
fn a_files_charge_is_the_same_whatever_ran_before_it_and_whatever_the_store_holds() {
    // The four are language files: the last needs a cache entry the third of
    // them fills, and the first is the batch's first file.
    let dir = Scratch::new("charge-order");
    for name in ["booleans", "blocks", "attune", "async-error-propagation"] {
        let text = std::fs::read_to_string(repo().join(format!("tests/lang/{name}.lisp")))
            .expect("read a language file");
        dir.write(&format!("{name}.lisp"), &text);
    }
    let subject = "async-error-propagation.lisp";
    let first = dir.path().join("first.db");
    let second = dir.path().join("second.db");
    let mut charges = Vec::new();
    for (db, before) in [
        (&first, "blocks.lisp"),
        (&second, "attune.lisp"),
        (&first, "attune.lisp"),
    ] {
        let out = run(
            dir.path(),
            db,
            &["--charge", "booleans.lisp", before, subject],
        );
        assert!(out.status.success(), "the run passes:\n{}", stderr(&out));
        charges.push(charge_of(db, subject));
    }
    assert!(
        charges[0].contains(":axis \"objects\""),
        "the charge reads the objects the run left live:\n{}",
        charges[0]
    );
    assert_eq!(
        charges[0].lines().count(),
        1,
        "and nothing else, until the other gauges stop following timing:\n{}",
        charges[0]
    );
    assert_eq!(
        charges[0], charges[1],
        "after another file, on a fresh store, the charge is the same"
    );
    assert_eq!(
        charges[0], charges[2],
        "and on a store that already holds the file's output"
    );
}

#[test]
fn a_charge_is_judged_against_the_rows_of_the_producer_elle_test() {
    let dir = bench(
        "judged",
        &[(
            "runner.lisp",
            vec![
                own("[\"f.lisp\" :objects 1000000"),
                own("[\"f.lisp\" :bytes 0"),
            ],
        )],
    );
    let db = dir.path().join("s.db");
    let out = run(dir.path(), &db, &["--charge", "f.lisp"]);
    assert!(
        !out.status.success(),
        "a stale pin and a missing row gate the run:\n{}",
        stderr(&out)
    );
    assert!(
        verdict(&db, "f.lisp", "objects").contains("stale"),
        "the charge reads far below the pin:\n{}",
        query(&db, "SELECT * FROM measurement")
    );
    assert!(
        verdict(&db, "f.lisp", "bytes").contains("missing"),
        "and a row of the file's that the charge never read is missing"
    );
}

#[test]
fn a_file_gated_on_every_tier_has_no_charge_and_its_row_is_left_alone() {
    // What a gated run leaves live follows what the box has installed:
    // tests/lang/zmq.lisp reads 3 objects fewer where libzmq is absent than
    // where its tests run, so a pin set on one box read stale on the other.
    let dir = bench(
        "gated",
        &[("runner.lisp", vec![own("[\"gated.lisp\" :objects 1000000")])],
    );
    dir.write(
        "gated.lisp",
        "(def why \"the library is absent\")\n(error {:error :gated :reason why})\n",
    );
    let db = dir.path().join("s.db");
    let out = run(dir.path(), &db, &["--charge", "gated.lisp"]);
    assert_eq!(
        statuses(&db),
        "{:status \"skip\"}\n",
        "the file skips on every tier"
    );
    assert_eq!(
        rows_of(&db, "gated.lisp"),
        "",
        "it records no reading, and its row is not missing"
    );
    assert!(
        out.status.success(),
        "so the row it never read leaves the run green:\n{}{}",
        stdout(&out),
        stderr(&out)
    );
}

#[test]
fn a_file_that_ran_on_any_tier_is_charged() {
    // A tier can refuse a file the same way on every box, so a skip on one
    // tier says nothing about what the box has installed. This file skips
    // with the JIT off and runs with it on.
    let dir = bench(
        "half",
        &[("runner.lisp", vec![own("[\"half.lisp\" :objects 1000000")])],
    );
    dir.write(
        "half.lisp",
        "(def jit (vm/config :jit))\n\
         (when (nil? jit) (error {:error :gated :reason \"no JIT on this tier\"}))\n\
         (assert (= (+ 1 1) 2) \"one and one\")\n",
    );
    let db = dir.path().join("s.db");
    let out = run(dir.path(), &db, &["--charge", "half.lisp"]);
    assert_eq!(
        statuses(&db),
        "{:status \"pass\"}\n{:status \"skip\"}\n",
        "the file passes on one tier and skips on the other"
    );
    assert!(
        verdict(&db, "half.lisp", "objects").contains("stale"),
        "its charge is read and judged:\n{}",
        rows_of(&db, "half.lisp")
    );
    assert!(
        !out.status.success(),
        "and the stale pin gates the run:\n{}",
        stderr(&out)
    );
}

#[test]
fn a_producers_rows_span_two_ledger_files_and_repin_moves_each_in_its_own() {
    let dir = bench(
        "spread",
        &[
            ("runner-1.lisp", vec![own("[\"f.lisp\" :objects 1000000")]),
            ("runner-2.lisp", vec![own("[\"g.lisp\" :objects 1000000")]),
        ],
    );
    let db = dir.path().join("s.db");
    let out = run(
        dir.path(),
        &db,
        &["--charge", "--repin", "f.lisp", "g.lisp", "h.lisp"],
    );
    let err = stderr(&out);
    for subject in ["f.lisp", "g.lisp"] {
        assert!(
            verdict(&db, subject, "objects").contains("stale"),
            "each file's row is judged:\n{err}"
        );
    }
    let one = std::fs::read_to_string(dir.path().join("tests/ledger/runner-1.lisp"))
        .expect("read runner-1");
    let two = std::fs::read_to_string(dir.path().join("tests/ledger/runner-2.lisp"))
        .expect("read runner-2");
    assert!(
        !one.contains("1000000") && !two.contains("1000000"),
        "each stale row moves in the file that holds it:\n{one}\n{two}\n{err}"
    );
    assert!(
        two.contains("[\"h.lisp\" :objects "),
        "the unledgered reading lands in the file that sorts last:\n{two}"
    );
    assert!(!one.contains("h.lisp"), "and in no other file:\n{one}");
}

#[test]
fn charge_refuses_isolate_and_an_ad_hoc_form() {
    let dir = bench("refuse", &[]);
    let db = dir.path().join("s.db");
    for args in [
        vec!["--charge", "--isolate", "", "f.lisp"],
        vec!["--charge", "-e", "(+ 1 1)"],
    ] {
        let out = run(dir.path(), &db, &args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?} is refused before anything runs:\n{}{}",
            stdout(&out),
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("--charge"),
            "and the refusal names --charge:\n{}",
            stderr(&out)
        );
    }
}
