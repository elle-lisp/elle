// audited: 2026-09-20
// The `elle` binary gives the program value's owning reference back, so a run
// that answers with a heap value leaves nothing behind.
// docs/impl/region/rules.md

use std::io::Write;
use std::process::{Command, Stdio};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The residue count out of the `--stats` teardown line, or a panic naming what
/// the run printed instead.
fn residue_of(stderr: &str, what: &str) -> usize {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("[stats] live regions after teardown: "))
        .unwrap_or_else(|| panic!("no [stats] residue line for {what}; stderr:\n{stderr}"));
    line.rsplit("teardown: ")
        .next()
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("unparseable residue line {line:?}"))
}

/// Run `elle --stats <args>` and answer its reported residue.
fn residue_after(args: &[&str], what: &str) -> usize {
    let out = Command::new(elle_binary())
        .arg("--stats")
        .args(args)
        .output()
        .expect("run elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "elle --stats {args:?} failed on {what}; stderr:\n{stderr}"
    );
    residue_of(&stderr, what)
}

/// Run `elle --stats -` with `source` on stdin and answer its reported residue.
fn residue_after_stdin(source: &str) -> usize {
    let mut child = Command::new(elle_binary())
        .args(["--stats", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(source.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait for elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "elle --stats - failed on {source}; stderr:\n{stderr}"
    );
    residue_of(&stderr, source)
}

/// One program value per heap shape a top-level form answers with: a list, a
/// string, an array, a struct, and a closure. A run whose last form is one of
/// these has no binding anywhere to root the value, so the binary's own release
/// is the only thing that can balance the return convention's mint.
const ANSWERS: [&str; 5] = [
    "(list 1 2 3)",
    "\"str\"",
    "[1 2 3]",
    "{:a 1}",
    "(fn [x] x)",
];

/// A run that answers with a heap value leaves nothing behind
/// (docs/impl/region/rules.md § "Teardown — every region frees").
///
/// The counter-factual is `(begin <answer> nil)`, measured beside it. It
/// allocates the very same value and drops it inside the program, where the
/// compiler's own release reclaims it. So a difference between the two readings
/// is the hand-off alone: not the allocation, not the shape, and not the size —
/// `(range 300)` reads what `(range 3)` reads.
#[test]
fn a_run_answering_with_a_heap_value_leaves_no_residue() {
    for answer in ANSWERS {
        let discarded = format!("(begin {answer} nil)");
        let dropped = residue_after(&[&format!("--eval:{discarded}")], &discarded);
        assert_eq!(
            dropped, 0,
            "{discarded}: a run that drops its value inside the program must \
             leave nothing"
        );
        let answered = residue_after(&[&format!("--eval:{answer}")], answer);
        assert_eq!(
            answered, 0,
            "{answer}: answering with the value left {answered} regions where \
             discarding it leaves {dropped} — the run kept the one owning \
             reference the return convention handed it"
        );
    }
}

/// The same claim on the two other ways a program reaches the binary: a file
/// argument, and stdin. One `run_source` serves all three, so a release that
/// reaches only one of them sits in the wrong place.
#[test]
fn a_file_or_stdin_run_answering_with_a_heap_value_leaves_no_residue() {
    let dir = crate::common::ScratchDir::new("program-value");
    let script = dir.join("answer.lisp");
    for answer in ANSWERS {
        std::fs::write(&script, format!("{answer}\n")).expect("write script");
        let path = script.to_str().expect("utf-8 path");
        assert_eq!(residue_after(&[path], answer), 0, "{answer}: run as a file");
        assert_eq!(residue_after_stdin(answer), 0, "{answer}: run from stdin");
    }
}
