// audited: 2026-11-25
// A CI step runs one make target, and a make target runs at most one `elle test` pass.
//
// docs/analysis/ci.md
//
// GitHub times a step and folds its log, so a step is the unit a reader finds
// a slow pass and the end of one pass by. These tests read every workflow's run
// lines, and ask `make --dry-run` what each suite target runs. Which job runs
// each pass is smoke_jobs.rs's.

use crate::common::{
    make_steps, passes, recording_targets, smoke_passes, workflow_files, workflow_jobs as jobs,
};
use std::collections::BTreeSet;

/// The targets that run one build's producer pass.
const PRODUCER_PASSES: [&str; 3] = [
    "smoke-producers",
    "smoke-pool-producers",
    "smoke-mlir-producers",
];

// A CI step runs one make target, and a target that ran several passes made
// one step of several: its time was their sum, and its log ran their batch
// summaries together with one `===` line between two passes
// (docs/analysis/ci.md).
//
// The counter-factual: the default build's `make smoke-impl` step ran four
// passes, the pool and MLIR jobs ran three in one step each, and the merge
// queue ran every pass of `make smoke` in one step.
#[test]
fn no_ci_step_runs_more_than_one_elle_test_pass() {
    let recording = recording_targets();
    let mut checked = 0;
    for (path, text) in workflow_files() {
        for (job, body) in jobs(&text) {
            for (target, vars) in make_steps(&body) {
                if !recording.contains(&target) {
                    continue;
                }
                checked += 1;
                let vars: Vec<&str> = vars.iter().map(String::as_str).collect();
                let n = passes(&target, &vars).len();
                assert_eq!(
                    n, 1,
                    "{path} job `{job}` runs `make {target}`, which is {n} elle test \
                     passes in one step"
                );
            }
        }
    }
    assert!(
        checked > 10,
        "found {checked} suite steps; the parse is broken, not the workflows"
    );
}

// A job runs its build's producer pass right after the build, before every
// other pass and the doctests: the pass is short, and a stale ledger row fails
// it as surely as a broken file fails a suite (docs/analysis/ci.md). `qa` and
// the tree's producers may come before it, because both are QA.
//
// The counter-factual: the default build's job ran its producers after three
// suite passes, so a stale row failed ten minutes into the job.
#[test]
fn every_job_runs_its_producer_pass_before_its_other_passes() {
    let mut later: BTreeSet<String> = recording_targets();
    later.extend(smoke_passes());
    for early in ["qa", "tree-producers"].into_iter().chain(PRODUCER_PASSES) {
        later.remove(early);
    }
    let mut checked = 0;
    for (path, text) in workflow_files() {
        for (job, body) in jobs(&text) {
            let steps: Vec<String> = make_steps(&body).into_iter().map(|(t, _)| t).collect();
            let Some(at) = steps
                .iter()
                .position(|t| PRODUCER_PASSES.contains(&t.as_str()))
            else {
                continue;
            };
            checked += 1;
            let before: Vec<&String> = steps[..at].iter().filter(|t| later.contains(*t)).collect();
            assert!(
                before.is_empty(),
                "{path} job `{job}` runs {before:?} before `make {}`",
                steps[at]
            );
        }
    }
    assert!(
        checked >= 6,
        "found {checked} jobs running a producer pass; the parse is broken, not the workflows"
    );
}

// `make smoke` runs its passes in `SMOKE_PASSES` order, so the producer pass
// leads there too, as it leads every job. The counter-factual: a local `make
// smoke` reaches a stale row half an hour after it starts.
#[test]
fn make_smoke_runs_the_producer_pass_first() {
    let passes = smoke_passes();
    assert_eq!(
        passes.first().map(String::as_str),
        Some("smoke-producers"),
        "`SMOKE_PASSES` does not lead with the producer pass: {passes:?}"
    );
}
