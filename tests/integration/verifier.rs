// audited: 2026-10-23
// Which processes run Cranelift's verifier: `elle` as asked, `elle test` and
// its children always, every Makefile recipe, and every CI job.
//
// docs/impl/verifier.md
// docs/config.md
//
// src/config/tests/verifier.rs pins what the parser reads. This file pins what
// the binary does with it: the setting reaches the VM a program reads it from,
// `elle test` turns it on whatever its environment says, and a child it starts
// inherits it. Every run clears the variable first, so the answer cannot come
// from the shell that runs `cargo test`.

use std::process::{Command, Output};

use crate::common::{workflow_files, ScratchDir};

const VAR: &str = "ELLE_JIT_VERIFY";

/// `elle` with `args` and `rest` after them, and `ELLE_JIT_VERIFY` set to
/// `var`, or unset for `None`.
fn spawn(args: &[&str], var: Option<&str>, rest: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elle"));
    command.env_remove(VAR).args(args).args(rest);
    if let Some(value) = var {
        command.env(VAR, value);
    }
    command.output().expect("spawn elle")
}

/// What `(vm/config :jit-verify)` reads in `elle ARGS`, with the variable at
/// `var`.
fn reads(args: &[&str], var: Option<&str>) -> String {
    let out = spawn(args, var, &["-e", "(println (vm/config :jit-verify))"]);
    assert!(
        out.status.success(),
        "elle {args:?} with {VAR}={var:?} exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn elle_does_not_verify_unless_asked() {
    assert_eq!(reads(&[], None), "false");
    assert_eq!(reads(&[], Some("")), "false", "an empty variable is unset");
}

#[test]
fn the_variable_turns_the_verifier_on_and_off() {
    assert_eq!(reads(&[], Some("on")), "true");
    assert_eq!(reads(&[], Some("off")), "false");
}

#[test]
fn the_flag_wins_over_the_variable() {
    assert_eq!(reads(&["--jit-verify=on"], None), "true");
    assert_eq!(reads(&["--jit-verify=off"], Some("on")), "false");
}

/// The counter-factual: a value read as off would let `ELLE_JIT_VERIFY=1`
/// run a whole suite unverified and report a pass.
#[test]
fn a_bad_value_stops_elle_before_the_program_runs() {
    let out = spawn(&[], Some("1"), &["-e", "(println :ran)"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "elle ran with {VAR}=1");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("ran"),
        "the program ran before the refusal"
    );
    assert!(
        stderr.contains(VAR) && stderr.contains('1'),
        "the refusal must name the variable and the value, got:\n{stderr}"
    );
}

#[test]
fn help_names_the_flag_and_the_variable() {
    let out = spawn(&["--help"], None, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--jit-verify") && stdout.contains(VAR),
        "--help must list --jit-verify and {VAR}, got:\n{stdout}"
    );
}

/// The assertion a test process makes about itself.
const VERIFIES: &str = "(assert (= (vm/config :jit-verify) true) \"this test process verifies\")";

/// `elle test` with `rest`, its store in `dir`, and the variable at `var`.
fn elle_test(dir: &ScratchDir, var: Option<&str>, rest: &[&str]) -> Output {
    let db = dir.join("verifier.db");
    let db = db.to_str().expect("a UTF-8 scratch path");
    let mut args = vec!["--db", db, "--timeout", "60000"];
    args.extend_from_slice(rest);
    spawn(&["test"], var, &args)
}

fn assert_passed(out: &Output, what: &str) {
    assert!(
        out.status.success(),
        "{what} exited {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The counter-factual for `off`: a runner that left the setting to its
/// environment would test the translator unverified whenever a shell exported
/// the user default.
#[test]
fn elle_test_verifies_whatever_its_environment_says() {
    for var in [None, Some("off")] {
        let dir = ScratchDir::new("verifier-runner");
        let out = elle_test(&dir, var, &["-e", VERIFIES]);
        assert_passed(&out, &format!("elle test with {VAR}={var:?}"));
    }
}

/// An `--isolate` child is a plain `elle FILE`, which reads only its
/// environment, so the runner has to hand the setting down.
#[test]
fn a_child_of_elle_test_verifies() {
    let dir = ScratchDir::new("verifier-child");
    let file = dir.join("child.lisp");
    std::fs::write(&file, format!("{VERIFIES}\n")).expect("write the child's file");
    let file = file.to_str().expect("a UTF-8 scratch path");
    let out = elle_test(&dir, None, &["--isolate", "", file]);
    assert_passed(&out, "elle test --isolate ''");
}

/// Every recipe sees the variable, so `make doctest` and `make smoke-plugins`,
/// which run `elle` on a file directly, verify as the suites do.
#[test]
fn the_makefile_exports_the_variable_to_every_recipe() {
    let out = Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove(VAR)
        .args([
            "--no-print-directory",
            "--eval",
            &format!("verifier-probe: ; @printenv {VAR}"),
            "verifier-probe",
        ])
        .output()
        .expect("run make");
    assert!(out.status.success(), "the probe recipe failed");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "on");
}

/// A CI step that calls `cargo test` runs no recipe, so the workflow has to
/// set the variable itself, for every job, and no job may set it otherwise.
#[test]
fn every_workflow_sets_the_variable_for_all_its_jobs() {
    let wanted = format!("  {VAR}: \"on\"");
    for (path, text) in workflow_files() {
        let top: Vec<&str> = text
            .lines()
            .skip_while(|line| *line != "env:")
            .skip(1)
            .take_while(|line| line.starts_with(' ') || line.is_empty())
            .collect();
        assert!(
            top.contains(&wanted.as_str()),
            "{path}: the top-level `env:` must hold `{}`",
            wanted.trim()
        );
        let settings = text
            .lines()
            .filter(|line| line.trim_start().starts_with(&format!("{VAR}:")))
            .count();
        assert_eq!(settings, 1, "{path}: a job sets {VAR} over the workflow's");
    }
}
