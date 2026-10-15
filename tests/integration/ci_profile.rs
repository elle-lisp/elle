// audited: 2026-10-15
// The Rust Tests jobs and `make test` build the `ci` profile: the dev profile with the workspace optimized.
//
// docs/analysis/ci.md
//
// The counter-factual: the jobs built the dev profile, so every `elle` process
// a test started ran at `opt-level = 0`, and the shared test binary took more
// than half an hour on a runner. Nothing failed; each job was only slower.

use crate::common::{job_steps, make_dry_run, repo_root, workflow_files, workflow_jobs};

/// The flag that selects the profile.
const PROFILE: &str = "--profile ci";

/// The lines of the `[profile.ci]` table in the root manifest.
fn ci_profile_table() -> Vec<String> {
    let text =
        std::fs::read_to_string(repo_root().join("Cargo.toml")).expect("read the root manifest");
    text.lines()
        .skip_while(|line| line.trim() != "[profile.ci]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .map(|line| line.split('#').next().unwrap_or("").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The profile keeps the dev profile's debug assertions, which compile the
/// region checks in. The trap: a profile that inherits `release` instead is
/// faster still, and runs the Rust suite with every region check compiled out.
#[test]
fn the_ci_profile_is_the_dev_profile_optimized() {
    let table = ci_profile_table();
    assert!(
        !table.is_empty(),
        "the root manifest defines no `[profile.ci]`, or defines it empty"
    );
    let has = |want: &str| table.iter().any(|line| line.replace(' ', "") == want);
    assert!(
        has("inherits=\"dev\""),
        "`[profile.ci]` must inherit the dev profile, got: {table:?}"
    );
    assert!(
        has("opt-level=2"),
        "`[profile.ci]` must build at opt-level 2, got: {table:?}"
    );
    assert!(
        !table
            .iter()
            .any(|line| line.starts_with("debug-assertions")),
        "`[profile.ci]` must keep the dev profile's debug assertions, got: {table:?}"
    );
}

/// A Rust Tests job runs `cargo test --test '*'`. Every `cargo test` in such a
/// job, the rig's and the property tests' included, builds the `ci` profile.
#[test]
fn every_rust_tests_job_builds_the_ci_profile() {
    let mut checked = 0;
    let mut missing: Vec<String> = Vec::new();
    for (path, text) in workflow_files() {
        for (job, body) in workflow_jobs(&text) {
            if !body.contains("cargo test --test '*'") {
                continue;
            }
            checked += 1;
            for step in job_steps(&body) {
                for line in step.lines() {
                    let line = line.trim_start().trim_start_matches("- ");
                    if line.starts_with("name:") || !line.contains("cargo test") {
                        continue;
                    }
                    if !line.contains(PROFILE) {
                        missing.push(format!("{path}: job `{job}`: {line}"));
                    }
                }
            }
        }
    }
    assert!(
        checked > 2,
        "found only {checked} Rust Tests jobs; the parse is broken, not the workflows"
    );
    assert!(
        missing.is_empty(),
        "these Rust Tests steps build the dev profile, not `{PROFILE}`:\n  {}",
        missing.join("\n  ")
    );
}

/// `make test` predicts the gate, so its integration and rig tests build the
/// profile the Rust Tests jobs build.
#[test]
fn make_test_builds_the_ci_profile() {
    let Some(recipe) = make_dry_run("test") else {
        panic!("`make --dry-run test` did not run");
    };
    for command in ["cargo test --test '*'", "cargo test -p elle-rig"] {
        let lines: Vec<&str> = recipe
            .lines()
            .filter(|line| line.starts_with(command))
            .collect();
        assert!(!lines.is_empty(), "`make test` runs no `{command}`");
        for line in lines {
            assert!(
                line.contains(PROFILE),
                "`make test` builds the dev profile, not `{PROFILE}`: {line}"
            );
        }
    }
}
