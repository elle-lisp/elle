// audited: 2026-10-05
// The valgrind producer reads nothing on a rig that is not a release build.
// tests/ratchet/overview.md
//
// The counter-factual: a ledger row's build names no profile, so a debug rig
// that ran the producer would judge its own memcheck counts against the
// release rows and fail on a difference that belongs to the profile.

mod common;

use common::{stderr, stdout};
use std::path::Path;
use std::process::Command;

#[cfg(debug_assertions)]
#[test]
fn a_debug_rig_gates_the_valgrind_producer_out() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rig sits in the repository");
    let out = Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .current_dir(root)
        .arg("tests/ratchet/valgrind.lisp")
        .output()
        .expect("spawn elle-rig");
    let both = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        out.status.success(),
        "a producer that gates itself out exits 0:\n{both}"
    );
    assert!(
        both.contains("SKIP (gated):") && both.contains("release"),
        "and says it skipped because the rig is no release build:\n{both}"
    );
    assert!(
        !stdout(&out).contains("measure "),
        "and prints no reading:\n{both}"
    );
}
