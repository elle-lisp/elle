// audited: 2026-11-22
// The rig runs Cranelift's verifier whatever its flags say, and leaves the
// environment it hands a child alone.
// docs/impl/verifier.md

mod common;

use common::Scratch;
use std::process::{Command, Output};

/// The variable that once carried the setting, which nothing reads now.
const VAR: &str = "ELLE_JIT_VERIFY";

/// The rig with `args`, in an environment without the old variable.
fn rig(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .env_remove(VAR)
        .args(args)
        .output()
        .expect("spawn elle-rig")
}

/// What `source` prints under the rig, with elle's `flags` ahead of it.
fn prints(flags: &[&str], source: &str) -> String {
    let mut args = flags.to_vec();
    args.extend(["-e", source]);
    let out = rig(&args);
    assert!(
        out.status.success(),
        "elle-rig {args:?} exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The counter-factual: a rig that let the flag decide would run the
/// implementation suite unverified for any caller that passed `off`.
#[test]
fn a_program_on_the_rig_verifies_whatever_its_flags_say() {
    for flags in [&[][..], &["--jit-verify=off"][..]] {
        assert_eq!(
            prints(flags, "(println (vm/config :jit-verify))"),
            "true",
            "elle-rig {flags:?}"
        );
    }
}

/// A program on the rig that starts `elle` names the flag itself. The
/// counter-factual: a rig that still set the variable would hand every child
/// a setting no process reads.
#[test]
fn the_rig_sets_no_verifier_variable() {
    assert_eq!(
        prints(&[], &format!("(println (sys/env \"{VAR}\"))")),
        "nil"
    );
}

#[test]
fn the_runner_on_the_rig_verifies() {
    let dir = Scratch::new("verifier");
    let db = dir.path().join("verifier.db");
    let out = rig(&[
        "test",
        "--db",
        db.to_str().expect("a UTF-8 scratch path"),
        "--timeout",
        "60000",
        "-e",
        "(assert (= (vm/config :jit-verify) true) \"the rig's runner verifies\")",
    ]);
    assert!(
        out.status.success(),
        "elle-rig test exited {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
