// audited: 2026-10-21
// The boot-image gate: the runner's flag, the target that runs the suite from an
// image, and when it checks bytecode parity.
//
// docs/impl/image/boot.md
// docs/analysis/ci.md
//
// Every part fails quietly, which is why each is pinned here. `elle test`
// partitions its argv: a flag it does not know is not refused, it is handed to
// the runner as a file path. And the gate target is shell text nothing
// compiles, so a target that drops the flag runs the language suite on an
// ordinary source boot and reports exactly the green a hydrated run reports.
//
// boot_parity.rs pins the parity check itself.

use crate::common::{make_dry_run, make_var};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The gate target's name, spelled once.
const GATE: &str = "smoke-boot-image";

/// What `make` says the gate target will run, without running it.
///
/// Asking `make` rather than reading the Makefile is what makes this a test of
/// the recipe as it expands: the directory, the binary and the runner flags all
/// arrive through variables, and a parser that resolved them itself would be a
/// second `make` that can disagree with the first.
fn gate_recipe() -> String {
    make_dry_run(GATE).unwrap_or_else(|| panic!("`make --dry-run {GATE}` failed; no such target"))
}

/// Every `--boot-image=` value the gate's recipe names. A value that closes a
/// quoted argument, such as `--isolate '--boot-image=DIR'`, ends before the quote.
fn boot_image_values(recipe: &str) -> Vec<String> {
    recipe
        .split("--boot-image=")
        .skip(1)
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches(['\'', '"'])
                .to_string()
        })
        .collect()
}

// The flag has to survive `elle test`'s argv partition. The runner is an Elle
// program handed everything the partition does not claim, so an unclaimed
// `--boot-image=DIR` becomes a file path: the run then fails on a file that
// is not there, and nothing ever hydrates. The counter-factual is the whole
// gate — point the language suite at an image through a flag the runner
// swallows, and every batch boots from source under a target that says
// otherwise.
#[test]
fn the_runner_boots_from_the_image_its_flag_names() {
    let dir = crate::common::ScratchDir::new("boot-image-runner");
    let flag = format!("--boot-image={}", dir.path().display());
    let file = dir.join("arithmetic.lisp");
    std::fs::write(&file, "(assert (= (+ 1 2) 3) \"the runner ran a file\")\n")
        .expect("write the file the runner runs");

    // Fill the cache: this start meets an empty directory, so it compiles the
    // three sources and stores what the run below has to hydrate.
    let stored = Command::new(elle_binary())
        .arg(&flag)
        .arg(&file)
        .output()
        .expect("run elle");
    assert!(
        stored.status.success(),
        "the storing run failed: {}",
        String::from_utf8_lossy(&stored.stderr)
    );

    let out = Command::new(elle_binary())
        .arg("test")
        .arg(&flag)
        .arg("--trace=boot")
        .args(["--timeout", "60000"])
        .arg("--db")
        .arg(dir.join("session.db"))
        .arg(&file)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "`elle test {flag}` did not pass the file it was given: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.starts_with("[trace:boot]") && l.contains("image-hydrate")),
        "`elle test {flag}` booted without hydrating the image, so the language suite \
         under the gate would run on a source boot: {stderr}"
    );
}

// A gate that greens whatever it is given gates nothing. The target names one
// directory, fills it, and hands the same one to every suite batch; a recipe
// that names two directories fills one and reads the other, which is a source
// boot with extra steps.
#[test]
fn the_gate_target_points_the_language_suite_at_one_image_directory() {
    let recipe = gate_recipe();
    let values = boot_image_values(&recipe);
    assert!(
        !values.is_empty(),
        "`make {GATE}` names no `--boot-image=`, so it runs the language suite on a \
         source boot:\n{recipe}"
    );
    let first = &values[0];
    assert!(
        values.iter().all(|v| v == first),
        "`make {GATE}` names more than one boot-image directory, so the one it \
         fills is not the one the language suite reads: {values:?}"
    );
    assert!(
        first.contains('/'),
        "`make {GATE}` points the language suite at `{first}` rather than at a \
         directory of its own"
    );

    // Every suite batch goes through the runner, which hands each file's child
    // its flags, and the child is the process whose boot the flag decides. A
    // recipe carrying the flag anywhere else proves nothing about the run that
    // matters.
    let elle = make_var("ELLE", &[]).expect("`make print-ELLE` did not run");
    let runner = format!("{elle} test");
    let batches: Vec<&str> = recipe
        .lines()
        .filter(|line| line.contains(&runner))
        .collect();
    assert!(
        !batches.is_empty(),
        "`make {GATE}` never runs `{runner}`, so it does not run the language suite:\n{recipe}"
    );
    for line in batches {
        assert!(
            line.contains("--boot-image="),
            "a suite batch in `make {GATE}` runs without the flag, so those \
             files boot from source: {line}"
        );
    }
}

// The trap `make check-wasm` already guards, in the boot image's shape: a
// binary that ignores `--boot-image=` accepts it and boots from source, and an
// image every start refuses is replaced and refused again. Either way the
// suite passes and the gate reports a hydration that never happened. So the
// target asks for the boot mark and fails without it.
//
// The counter-factual: drop the proof, break the warm cache's hydration, and
// this gate stays green while every other suite job also stays green — no
// other job boots from the warm cache.
#[test]
fn the_gate_target_proves_the_image_hydrated_before_it_runs_the_language_suite() {
    let recipe = gate_recipe();
    assert!(
        recipe.contains("--trace=boot"),
        "`make {GATE}` runs no `--trace=boot` start, so it cannot tell a \
         hydrated boot from a source boot:\n{recipe}"
    );
    assert!(
        recipe.contains("image-hydrate"),
        "`make {GATE}` never reads the `image-hydrate` mark, so a binary that \
         ignored `--boot-image=` would pass it:\n{recipe}"
    );
}

// `make smoke-boot-image` runs the parity check before the language suite. A
// corpus that answers alike under an image boot can still have compiled to
// other bytecode, and the suite cannot see that; the dumps can. The recipe is
// read as `make` expands it, so a prerequisite dropped from the target fails
// here rather than leaving a green job that compares nothing.
#[test]
fn the_gate_target_compares_bytecode_before_it_runs_the_language_suite() {
    let recipe = gate_recipe();
    let elle = make_var("ELLE", &[]).expect("`make print-ELLE` did not run");
    let runner = format!("{elle} test");
    let parity = recipe
        .find("scripts/bytecode-golden parity")
        .unwrap_or_else(|| panic!("`make {GATE}` never runs the bytecode parity check:\n{recipe}"));
    let suite = recipe
        .find(&runner)
        .unwrap_or_else(|| panic!("`make {GATE}` never runs `{runner}`:\n{recipe}"));
    assert!(
        parity < suite,
        "`make {GATE}` runs the language suite before the parity check:\n{recipe}"
    );
}
