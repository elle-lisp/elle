// audited: 2026-10-06
// The valgrind producer reads nothing on a rig that is not a release build.
// tests/ratchet/overview.md
//
// The counter-factual: a ledger row's build names no profile, so a debug rig
// that ran the producer would judge its own memcheck counts against the
// release rows and fail on a difference that belongs to the profile. The
// profile is checked before valgrind is looked for, so the reason is the same
// on a box without valgrind, as the macOS runner is.

mod common;

use common::{stderr, stdout};
use std::path::Path;
use std::process::{Command, Output};

/// The rig run on the producer from the repository root. An empty `PATH`
/// stands for a box with no valgrind; `None` inherits the caller's.
fn run_producer(path: Option<&str>) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rig sits in the repository");
    let mut rig = Command::new(env!("CARGO_BIN_EXE_elle-rig"));
    rig.current_dir(root).arg("tests/ratchet/valgrind.lisp");
    if let Some(path) = path {
        rig.env("PATH", path);
    }
    rig.output().expect("spawn elle-rig")
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_rig_gates_the_valgrind_producer_out() {
    for (path, the_box) in [(None, "this box"), (Some(""), "a box with no valgrind")] {
        let out = run_producer(path);
        let both = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            out.status.success(),
            "on {the_box}, a producer that gates itself out exits 0:\n{both}"
        );
        assert!(
            both.contains("SKIP (gated):") && both.contains("release"),
            "on {the_box}, it says it skipped because the rig is no release build:\n{both}"
        );
        assert!(
            !stdout(&out).contains("measure "),
            "on {the_box}, it prints no reading:\n{both}"
        );
    }
}
