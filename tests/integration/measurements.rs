// audited: 2026-10-05
// A run under `elle test` has no build: it records no reading, judges none,
// gates on none, and refuses to re-pin.
//
// docs/ratchet.md
// docs/test-store.md
//
// The counter-factual: a row with no :build belonged to every build that had
// none of its own, so `elle test`, the runner a developer iterates with, judged
// a pool or MLIR reading against the reference build's footprint and failed
// it. `elle` names no build, so a run under it has no rows to judge against.
// The runner's cases that need a build are the rig's (rig/tests).

use crate::common::query;
use std::process::{Command, Output};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A producer of several forms, the whole-file shape the runner runs in a
/// worker under each JIT policy, that reads one number.
const PRODUCER: &str = "(def r ((import \"std/ratchet\")))\n\
                        (def answer 42)\n\
                        (r:read \"answer\" :count answer)\n";

/// The row every case pins: 41 against a reading of 42, a regression on any
/// build that judged it.
const ROWS: &str = "[\"answer\" :count 41]";

/// A scratch working directory holding the producer, its ledger under
/// `tests/ledger`, and a session DB of its own.
struct Bench {
    dir: crate::common::ScratchDir,
}

impl Bench {
    fn new(tag: &str) -> Bench {
        let dir = crate::common::ScratchDir::new(&format!("measure-{tag}"));
        std::fs::write(dir.join("producer.lisp"), PRODUCER).expect("write the producer");
        let ledger = dir.join("tests/ledger");
        std::fs::create_dir_all(&ledger).expect("create the ledger dir");
        std::fs::write(
            ledger.join("producer.lisp"),
            format!("(elle/epoch 13)\n(producer \"producer.lisp\")\n{ROWS}\n"),
        )
        .expect("write the ledger");
        Bench { dir }
    }

    fn ledger(&self) -> String {
        std::fs::read_to_string(self.dir.join("tests/ledger/producer.lisp"))
            .expect("read the ledger")
    }

    /// `elle test ARGS producer.lisp` from the scratch directory, whose
    /// `tests/ledger` a run with a build would judge against.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(elle_binary())
            .arg("test")
            .args(args)
            .args(["--timeout", "60000"])
            .arg("--db")
            .arg(self.dir.join("s.db"))
            .arg("producer.lisp")
            .current_dir(self.dir.path())
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run elle test")
    }

    fn measurements(&self) -> String {
        query(
            &self.dir.join("s.db"),
            "SELECT count(*) AS c FROM measurement",
        )
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_run_with_no_build_records_no_reading_and_gates_on_none() {
    let b = Bench::new("nobuild");
    let out = b.run(&[]);
    assert!(
        out.status.success(),
        "a reading that would regress gates nothing with no build to judge it:\n{}",
        stderr(&out)
    );
    let count = b.measurements();
    assert!(
        count.contains(":c 0"),
        "no measurement row is written, got:\n{count}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("no build"),
        "the summary says the readings were neither recorded nor judged, so a \
         green run does not read as a passed gate:\n{err}"
    );
}

#[test]
fn an_isolated_child_of_elle_test_records_none_either() {
    let b = Bench::new("nobuild-child");
    let out = b.run(&["--isolate", ""]);
    assert!(out.status.success(), "the child passes:\n{}", stderr(&out));
    let count = b.measurements();
    assert!(
        count.contains(":c 0"),
        "a child of a run with no build records nothing, got:\n{count}"
    );
}

#[test]
fn a_charge_with_no_build_records_none_and_says_so() {
    // The charge is the runner's own reading, never a line a file printed,
    // and a run with no build records it no more than it records a line.
    let b = Bench::new("nobuild-charge");
    let out = b.run(&["--charge"]);
    assert!(
        out.status.success(),
        "the charge gates nothing with no build to judge it:\n{}",
        stderr(&out)
    );
    let count = b.measurements();
    assert!(
        count.contains(":c 0"),
        "no measurement row is written, got:\n{count}"
    );
    assert!(
        stderr(&out).contains("no build"),
        "and the summary says the readings were neither recorded nor judged:\n{}",
        stderr(&out)
    );
}

#[test]
fn repin_with_no_build_refuses_and_leaves_the_ledger_alone() {
    let b = Bench::new("nobuild-repin");
    let before = b.ledger();
    let out = b.run(&["--repin"]);
    assert!(
        !out.status.success(),
        "--repin with no build refuses:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("no build") && err.contains("elle-rig test"),
        "and says why, and what to run instead:\n{err}"
    );
    assert_eq!(
        b.ledger(),
        before,
        "the ledger is byte for byte what it was"
    );
}

#[test]
fn elle_names_no_build() {
    // The rig registers `elle/build` on the runtimes it builds; a user build
    // has no such primitive. A test that pins the name's absence guards
    // against the primitive moving into the shipped runtime.
    let out = Command::new(elle_binary())
        .args(["-e", "(elle/build)"])
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e");
    let err = stderr(&out);
    assert!(
        !out.status.success() && err.contains("undefined variable") && err.contains("elle/build"),
        "`elle/build` is an undefined variable under elle:\n{err}"
    );
}
