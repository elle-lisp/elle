// audited: 2026-10-23
// The rig runs Cranelift's verifier whatever its environment says, and hands
// the setting to every process it starts.
// docs/impl/verifier.md

mod common;

use common::Scratch;
use std::process::{Command, Output};

const VAR: &str = "ELLE_JIT_VERIFY";

/// The rig with `args`, and `ELLE_JIT_VERIFY` set to `var`, or unset for
/// `None`.
fn rig(args: &[&str], var: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elle-rig"));
    command.env_remove(VAR).args(args);
    if let Some(value) = var {
        command.env(VAR, value);
    }
    command.output().expect("spawn elle-rig")
}

/// What `source` prints under the rig, with the variable at `var`.
fn prints(source: &str, var: Option<&str>) -> String {
    let out = rig(&["-e", source], var);
    assert!(
        out.status.success(),
        "elle-rig -e {source:?} with {VAR}={var:?} exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The counter-factual: a rig that left the setting to its environment would
/// run the implementation suite unverified from any shell that did not export
/// the variable, and from one that exported `off`.
#[test]
fn a_program_on_the_rig_verifies_whatever_the_variable_says() {
    for var in [None, Some("off")] {
        assert_eq!(prints("(println (vm/config :jit-verify))", var), "true");
    }
}

/// A program on the rig that starts `elle` hands it the setting through the
/// environment, as `elle test` does for its children.
#[test]
fn a_program_on_the_rig_hands_the_setting_down() {
    assert_eq!(
        prints(&format!("(println (sys/env \"{VAR}\"))"), None),
        "on"
    );
}

#[test]
fn the_runner_on_the_rig_verifies() {
    let dir = Scratch::new("verifier");
    let db = dir.path().join("verifier.db");
    let out = rig(
        &[
            "test",
            "--db",
            db.to_str().expect("a UTF-8 scratch path"),
            "--timeout",
            "60000",
            "-e",
            "(assert (= (vm/config :jit-verify) true) \"the rig's runner verifies\")",
        ],
        Some("off"),
    );
    assert!(
        out.status.success(),
        "elle-rig test exited {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
