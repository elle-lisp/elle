// audited: 2026-09-29
// The rig answers elle's subcommands as elle does, so a program that runs its own executable runs under the rig.
// rig/overview.md
//
// The trap: `(elle/executable)` names the running binary, which on the rig is
// elle-rig. The semver tool's tests run `(elle/executable) semver …`, and the
// implementation suite's eager pass runs them on the rig. A rig that read the
// subcommand as a program path failed them there and nowhere else.

mod common;

use common::{rig, stderr, stdout, Scratch};

/// A file `elle fmt` leaves as it is.
const FORMATTED: &str = "(elle/epoch 13)\n(def x 1)\n";

/// The same file, spaced the way `elle fmt` would rewrite.
const UNFORMATTED: &str = "(elle/epoch 13)\n(def    x\n 1)\n";

// Both outcomes, so a rig that answered every subcommand with success would
// fail the second arm.
#[test]
fn fmt_checks_a_file_on_the_rig() {
    let dir = Scratch::new("fmt");
    let good = dir.write("good.lisp", FORMATTED);
    let bad = dir.write("bad.lisp", UNFORMATTED);
    let (good, bad) = (good.to_str().unwrap(), bad.to_str().unwrap());

    let out = rig(&["fmt", "--check", good]);
    assert!(
        out.status.success(),
        "`elle-rig fmt --check` rejected a formatted file: {}",
        stderr(&out)
    );

    let out = rig(&["fmt", "--check", bad]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "`elle-rig fmt --check` passed a file fmt would rewrite: {}{}",
        stdout(&out),
        stderr(&out)
    );
}

// The runner is the subcommand a suite pass drives, and it records into the
// store it is handed.
#[test]
fn the_runner_runs_on_the_rig() {
    let dir = Scratch::new("runner");
    let file = dir.write(
        "passes.lisp",
        "(elle/epoch 13)\n(assert (= (+ 1 2) 3) \"sum\")\n",
    );
    let db = dir.path().join("session.db");
    let out = rig(&["test", "--db", db.to_str().unwrap(), file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "`elle-rig test` did not pass a passing file: {}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        db.exists(),
        "`elle-rig test` recorded nothing at {}",
        db.display()
    );
}
