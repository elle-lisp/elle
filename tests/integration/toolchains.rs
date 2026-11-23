// audited: 2026-11-22
// Every CI job installs the toolchain its commands call: rustfmt and clippy for the command line's tests, all of QA's for `make qa`.
//
// docs/analysis/ci.md
// CONTRIBUTING.md
//
// The command line's tests run the real commit hook, and the hook calls
// `rustfmt` and `cargo clippy` on a staged `.rs` file. A job whose toolchain
// lacks either fails those tests with "'rustfmt' is not installed for the
// toolchain", which names the runner and not the code.

use crate::common::{job_steps, make_dry_run, make_expand, workflow_files, workflow_jobs};
use std::collections::{BTreeMap, BTreeSet};

/// The components every job that runs the command line's tests must install.
const REQUIRED: [&str; 2] = ["rustfmt", "clippy"];

/// Does a make target in this job run the command line's tests? Each answer
/// is cached in `seen` by target, because a dry run of one costs a `make`.
fn runs_cli_tests(body: &str, seen: &mut BTreeMap<String, bool>) -> bool {
    make_targets(body).into_iter().any(|target| {
        *seen.entry(target.clone()).or_insert_with(|| {
            make_dry_run(&target).is_some_and(|recipe| recipe.contains("tests/cli/"))
        })
    })
}

/// The list one key names in the job's `dtolnay/rust-toolchain` step, or
/// `None` when the job has no such step.
fn toolchain_list(body: &str, key: &str) -> Option<BTreeSet<String>> {
    let step = job_steps(body)
        .into_iter()
        .find(|step| step.contains("uses: dtolnay/rust-toolchain"))?;
    let key = format!("{key}:");
    Some(
        step.lines()
            .filter_map(|line| line.trim_start().strip_prefix(key.as_str()))
            .flat_map(|list| list.split(','))
            .map(|c| c.trim().trim_matches(['"', '\'']).to_string())
            .filter(|c| !c.is_empty())
            .collect(),
    )
}

/// The components named in the job's `dtolnay/rust-toolchain` step.
fn toolchain_components(body: &str) -> Option<BTreeSet<String>> {
    toolchain_list(body, "components")
}

// The trap: a job on `dtolnay/rust-toolchain@stable` passes with no
// `components` line, because the runner image already carries stable with
// rustfmt and clippy. A toolchain the image lacks arrives in rustup's minimal
// profile without them.
//
// The counter-factual: check only the jobs whose toolchain is not stable, and
// the stable jobs keep a dependency on the image that no test can see.
#[test]
fn every_job_that_runs_the_command_lines_tests_installs_rustfmt_and_clippy() {
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    let mut checked = 0;
    let mut missing: Vec<String> = Vec::new();
    for (path, text) in workflow_files() {
        for (job, body) in workflow_jobs(&text) {
            if !runs_cli_tests(&body, &mut seen) {
                continue;
            }
            checked += 1;
            let Some(components) = toolchain_components(&body) else {
                missing.push(format!("{path}: job `{job}` installs no toolchain"));
                continue;
            };
            let absent: Vec<&str> = REQUIRED
                .into_iter()
                .filter(|c| !components.contains(*c))
                .collect();
            if !absent.is_empty() {
                missing.push(format!(
                    "{path}: job `{job}` runs the command line's tests without {absent:?} in \
                     its toolchain step's `components`"
                ));
            }
        }
    }

    // The default build's rig job, AArch64 Smoke, macOS Smoke (rig) and the
    // merge queue all run `make smoke-impl`. A parse that found fewer asserted
    // nothing.
    assert!(
        checked > 3,
        "found only {checked} jobs that run the command line's tests; the \
         parse is broken, not the workflows"
    );
    assert!(
        missing.is_empty(),
        "the commit hook's tests call rustfmt and clippy, so these jobs fail \
         on a toolchain that lacks them:\n  {}",
        missing.join("\n  ")
    );
}

/// The variables `qa`'s `--all-features` clippy and rustdoc need to find LLVM 22
/// for the MLIR tier (docs/impl/mlir.md).
const QA_ENV: [&str; 3] = [
    "MLIR_SYS_220_PREFIX",
    "TABLEGEN_220_PREFIX",
    "LIBCLANG_PATH",
];

/// The make targets a job's steps run, as the word after `make `.
fn make_targets(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: make "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

// A job reaches `qa` when a make target it runs starts it, as `make smoke`
// does. `qa` builds every feature, and `make crosscheck` inside it prints
// SKIPPED and passes over a target the toolchain lacks.
//
// The trap: the merge queue ran `make smoke` on a bare stable toolchain. Once
// `smoke` started `qa`, the cross-checks there would have skipped in silence,
// and clippy would have failed on an MLIR tier with no LLVM to build against.
// The counter-factual: with no job reaching `qa`, this test finds none.
#[test]
fn every_job_that_reaches_make_qa_carries_the_qa_toolchain() {
    let mut reaches_qa: BTreeMap<String, bool> = BTreeMap::new();
    let mut checked = 0;
    let mut missing: Vec<String> = Vec::new();
    let cross = [make_expand("CROSS_TARGET"), make_expand("ANDROID_TARGET")];
    for (path, text) in workflow_files() {
        for (job, body) in workflow_jobs(&text) {
            let runs_qa = make_targets(&body).into_iter().any(|target| {
                *reaches_qa.entry(target.clone()).or_insert_with(|| {
                    make_dry_run(&target).is_some_and(|recipe| recipe.contains("cargo fmt --check"))
                })
            });
            if !runs_qa {
                continue;
            }
            checked += 1;
            let components = toolchain_components(&body).unwrap_or_default();
            let targets = toolchain_list(&body, "targets").unwrap_or_default();
            for want in REQUIRED {
                if !components.contains(want) {
                    missing.push(format!("{path}: job `{job}` lacks the component {want}"));
                }
            }
            for want in &cross {
                if !targets.contains(want) {
                    missing.push(format!("{path}: job `{job}` lacks the target {want}"));
                }
            }
            for var in QA_ENV {
                if !body.contains(&format!("{var}:")) {
                    missing.push(format!("{path}: job `{job}` sets no {var}"));
                }
            }
        }
    }
    assert!(
        checked > 0,
        "no job runs a make target that starts `qa`, so the merge queue runs no \
         QA on the merged result"
    );
    assert!(
        missing.is_empty(),
        "these jobs run `make qa` on a toolchain that cannot run all of it:\n  {}",
        missing.join("\n  ")
    );
}
