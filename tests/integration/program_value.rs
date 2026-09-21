// audited: 2026-09-20
// The `elle` binary gives the program value's owning reference back, so a run
// that answers with a heap value leaves nothing behind.
//
// A script run answers once; a REPL session answers form by form, so the same
// defect reads as a slope there rather than a fixed cost.
//
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

/// Drive the binary's REPL over `session` and answer what it printed: stdout,
/// then stderr. No argument names a source, so the binary reads the prompt —
/// the entry path `residue_after` cannot reach, because every other one runs
/// through `run_source`.
fn repl(session: &str) -> (String, String) {
    let mut child = Command::new(elle_binary())
        .arg("--stats")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(session.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait for elle");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "the REPL failed on:\n{session}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    (stdout, stderr)
}

/// The residue a REPL session reports.
fn repl_residue(session: &str) -> usize {
    residue_of(&repl(session).1, session)
}

/// `n` copies of `form`, one per line: a session of `n` forms that each answer
/// with a value of the same shape.
fn repl_session(form: &str, n: usize) -> String {
    format!("{form}\n").repeat(n)
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

/// A REPL session leaves nothing behind either, however many forms it runs. A
/// prompt form that binds nothing hands its value to the REPL and to nobody
/// else, so the REPL owes the release the run path owes.
///
/// The counter-factual is the length: a session of `n` forms and one of `2n`
/// must report the same number. A host that keeps the run's reference leaks one
/// region per RUN, which a longer session cannot see — the REPL runs a form per
/// line, so the same defect here is a slope, and the two readings separate it
/// from any fixed cost the session pays once.
#[test]
fn a_repl_form_answering_with_a_heap_value_leaves_no_residue() {
    for answer in ANSWERS {
        let short = repl_residue(&repl_session(answer, 3));
        let long = repl_residue(&repl_session(answer, 6));
        assert_eq!(
            short, long,
            "{answer}: 3 forms left {short} regions and 6 left {long} — the \
             session keeps a reference per form"
        );
        assert_eq!(
            short, 0,
            "{answer}: the session left {short} regions — the REPL kept the \
             owning reference the return convention handed each form's value"
        );
    }
}

/// A `def` whose value is a heap value leaves nothing behind. The binding keeps
/// the value for the session, so it holds a reference of its own, and the
/// program value the REPL printed is released like any other.
#[test]
fn a_repl_def_leaves_no_residue() {
    for answer in ANSWERS {
        let session = format!("(def bound {answer})\nbound\n");
        assert_eq!(
            repl_residue(&session),
            0,
            "{session}: the session left a region behind",
        );
    }
}

/// A destructuring `def` compiles to the `def` followed by a tuple of its leaf
/// names, so the program value is that tuple and each leaf is an element of it.
/// The REPL releases the tuple and keeps the leaves, which is the one shape
/// needing both halves of the hand-off at once.
#[test]
fn a_repl_destructuring_def_leaves_no_residue() {
    for session in DESTRUCTURING_SESSIONS {
        assert_eq!(
            repl_residue(session),
            0,
            "{session}: the session left a region behind",
        );
    }
}

/// A destructured binding is readable for the rest of the session, after the
/// REPL has released the tuple it came out of.
///
/// The counter-factual is `a_repl_destructuring_def_leaves_no_residue` beside
/// it, which cannot see this: registering each leaf against the reference the
/// tuple holds reads as clean at teardown, and frees the leaves with the tuple
/// — earlier, and while the session still names them. This reads what the
/// binding says afterwards, so a leaf freed under it answers with a debug-build
/// panic or a wrong string rather than a clean census.
#[test]
fn a_destructured_repl_binding_outlives_the_tuple_it_came_from() {
    let (out, err) = repl(
        "(def [a b] [\"pp\" \"qq\"])\n\
         (def {:x c :y d} {:x \"rr\" :y \"ss\"})\n\
         (string/upcase a)\n\
         (string/upcase b)\n\
         (string/upcase c)\n\
         (string/upcase d)\n",
    );
    for expected in ["\"PP\"", "\"QQ\"", "\"RR\"", "\"SS\""] {
        assert!(
            out.contains(expected),
            "the session never printed {expected}; stdout:\n{out}\nstderr:\n{err}"
        );
    }
}

/// The two destructuring shapes the REPL takes apart, each binding heap values
/// so the leaves occupy regions of their own.
const DESTRUCTURING_SESSIONS: [&str; 2] = [
    "(def [a b] [\"pp\" \"qq\"])\n",
    "(def {:x a :y b} {:x \"rr\" :y \"ss\"})\n",
];

/// A form the REPL deferred and later resolved leaves nothing behind. Neither
/// path prints its value, so neither release can ride on the print: the single
/// retry runs the form alone, and the batch compiles the whole group as one
/// letrec whose trailing tuple is the program value.
#[test]
fn a_repl_resolving_a_deferred_form_leaves_no_residue() {
    let forward = "(defn ahead [] (behind))\n\
                   (defn behind [] \"s\")\n\
                   (ahead)\n";
    let mutual = "(defn ping [n] (if (= n 0) \"done\" (pong (- n 1))))\n\
                  (defn pong [n] (if (= n 0) \"done\" (ping (- n 1))))\n\
                  (ping 4)\n";
    assert_eq!(
        repl_residue(forward),
        0,
        "the individual retry left a region behind",
    );
    assert_eq!(
        repl_residue(mutual),
        0,
        "the batch letrec left a region behind",
    );
}
