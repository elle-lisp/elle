// audited: 2026-10-05
// `elle-rig test --repin` moves the rows of the run's build to what it read, adopts what had none, and refuses a regression.
// docs/ratchet.md
// docs/test-cli.md
//
// The counter-factual: a pin moved by hand is how a pin stays loose, and a
// build that never had rows had no way to join the ratchet but a hand-written
// ledger. Each test writes its rows for the key this rig answers.

mod common;

use common::ratchet::*;
use common::stderr;

#[test]
fn repin_moves_a_stale_pin_and_keeps_the_comment_above_it() {
    // Moving a pin by hand is how a pin stays loose. The tool moves the token
    // and nothing else, so the comment survives.
    let b = Bench::new(
        "loose",
        &[
            "# the answer, pinned the day it was accepted".to_string(),
            own("[\"answer\" :count 43"),
        ],
        PRODUCER,
    );
    let out = b.run(&["--repin"]);
    let err = stderr(&out);
    assert!(
        err.contains("repin") && err.contains("answer") && err.contains("42"),
        "the tool prints the row it moved:\n{err}"
    );
    let text = b.ledger();
    assert!(
        text.contains(&own("[\"answer\" :count 42")),
        "the pin took the new reading, got:\n{text}"
    );
    assert!(
        text.contains("# the answer, pinned the day it was accepted\n"),
        "and the comment above the row survived, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "a run against the moved ledger gates green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_adopts_an_unledgered_reading_as_a_row() {
    let b = Bench::new(
        "adopt",
        &[own("[\"answer\" :count 42")],
        "(def r ((import \"std/ratchet\")))\n\
         (r:read \"answer\" :count 42)\n\
         (r:read \"extra\" :count 7)\n",
    );
    let out = b.run(&["--repin"]);
    assert!(
        stderr(&out).contains("extra"),
        "the tool prints the row it adopted:\n{}",
        stderr(&out)
    );
    let text = b.ledger();
    assert!(
        text.ends_with(&format!("{}\n", adopted("[\"extra\" :count 7"))),
        "the reading is a row of this build after the last one, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "and the adopted row judges the next run green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_adopts_a_growth_reading_as_a_growth_floor() {
    // The instrument's own live-growth reading carries its class and its
    // floor, so the adopted row is a floor and not a pin at 1.0.
    let b = Bench::new(
        "floor",
        &[own("[\"dropped\" :objects 0")],
        "(def r ((import \"std/ratchet\")))\n\
         (r:delta \"dropped\" (fn [] {:x 1}) :on [r:objects] :n 50)\n",
    );
    b.run(&["--repin"]);
    let text = b.ledger();
    assert!(
        text.contains(&adopted(
            "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth"
        )),
        "the discriminator is adopted as a growth floor, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "and the next run gates green:\n{}",
        stderr(&again)
    );
}

#[test]
fn repin_refuses_a_regression_and_leaves_the_ledger_alone() {
    let b = Bench::new("refuse", &[own("[\"answer\" :count 41")], PRODUCER);
    let before = b.ledger();
    let out = b.run(&["--repin"]);
    assert!(
        !out.status.success(),
        "the regression still gates:\n{}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("refuse") && err.contains("answer"),
        "the tool says which row it refused to move:\n{err}"
    );
    assert_eq!(
        b.ledger(),
        before,
        "and the ledger is byte for byte what it was"
    );
}

#[test]
fn repin_on_a_build_with_no_rows_adopts_every_reading() {
    // How a build joins the ratchet: its first --repin writes a row of its
    // own for every reading, and another build's row is left as it was.
    let b = Bench::new("join", &[foreign("[\"answer\" :count 41")], PRODUCER);
    let out = b.run(&["--repin"]);
    assert!(
        out.status.success(),
        "a build with no rows gates nothing:\n{}",
        stderr(&out)
    );
    let text = b.ledger();
    assert!(
        text.contains(&adopted("[\"answer\" :count 42")),
        "the reading is adopted as a row of this build, got:\n{text}"
    );
    assert!(
        text.contains(&foreign("[\"answer\" :count 41")),
        "and the other build's row is untouched, got:\n{text}"
    );
    let again = b.run(&[]);
    assert!(
        again.status.success(),
        "the next run judges the adopted rows green:\n{}",
        stderr(&again)
    );
    assert!(
        b.rows().contains(":verdict \"ok\""),
        "against this build's rows:\n{}",
        b.rows()
    );
}
