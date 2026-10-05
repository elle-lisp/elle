// audited: 2026-10-04
// Every CI job that runs `cargo test` installs the rustfmt and clippy that the pre-commit tests call.
//
// docs/analysis/ci.md
// CONTRIBUTING.md
//
// The pre-commit tests run the real hook, and the hook calls `rustfmt` and
// `cargo clippy` on a staged `.rs` file. A job whose toolchain lacks either
// fails those tests with "'rustfmt' is not installed for the toolchain", which
// names the runner and not the code.

use crate::common::{job_steps, workflow_files, workflow_jobs};
use std::collections::BTreeSet;

/// The components every job that runs `cargo test` must install.
const REQUIRED: [&str; 2] = ["rustfmt", "clippy"];

/// Does this step run `cargo test`? A step's `name:` may spell the command it
/// labels, so that line does not count.
fn runs_cargo_test(step: &str) -> bool {
    step.lines().any(|line| {
        let line = line.trim_start().trim_start_matches("- ");
        !line.starts_with("name:") && line.contains("cargo test")
    })
}

/// The components named in the job's `dtolnay/rust-toolchain` step, or `None`
/// when the job has no such step.
fn toolchain_components(body: &str) -> Option<BTreeSet<String>> {
    let step = job_steps(body)
        .into_iter()
        .find(|step| step.contains("uses: dtolnay/rust-toolchain"))?;
    Some(
        step.lines()
            .filter_map(|line| line.trim_start().strip_prefix("components:"))
            .flat_map(|list| list.split(','))
            .map(|c| c.trim().trim_matches(['"', '\'']).to_string())
            .filter(|c| !c.is_empty())
            .collect(),
    )
}

// The trap: a job on `dtolnay/rust-toolchain@stable` passes with no
// `components` line, because the runner image already carries stable with
// rustfmt and clippy. A toolchain the image lacks, such as the weekly beta and
// nightly, arrives in rustup's minimal profile without them.
//
// The counter-factual: check only the jobs whose toolchain is not stable, and
// the stable jobs keep a dependency on the image that no test can see.
#[test]
fn every_job_that_runs_cargo_test_installs_rustfmt_and_clippy() {
    let mut checked = 0;
    let mut missing: Vec<String> = Vec::new();
    for (path, text) in workflow_files() {
        for (job, body) in workflow_jobs(&text) {
            if !job_steps(&body).iter().any(|step| runs_cargo_test(step)) {
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
                    "{path}: job `{job}` runs `cargo test` without {absent:?} in its \
                     toolchain step's `components`"
                ));
            }
        }
    }

    // The QA job, the three Rust Tests jobs and the weekly toolchain matrix
    // all run `cargo test`. A parse that found fewer asserted nothing.
    assert!(
        checked > 3,
        "found only {checked} jobs that run `cargo test`; the parse is broken, \
         not the workflows"
    );
    assert!(
        missing.is_empty(),
        "the pre-commit tests call rustfmt and clippy, so these jobs fail on \
         a toolchain that lacks them:\n  {}",
        missing.join("\n  ")
    );
}
