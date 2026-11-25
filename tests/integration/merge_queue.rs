// audited: 2026-11-25
// The merge queue runs QA and each pass a merge can break once, in jobs that start together behind one gate.
//
// docs/analysis/queue.md
//
// The queue checks a branch against the main it lands on, after the branch
// passed every pull-request job. These tests hold the workflow to what the
// document says it runs, and to the shape that keeps it short.

use crate::common::{enforced_results, job_needs, runs_target, workflow, workflow_jobs};
use std::collections::BTreeSet;

const WORKFLOW: &str = ".github/workflows/merge-queue.yml";

/// The gate job: the one whose result branch protection requires.
const GATE: &str = "all-checks";

/// What the queue runs on the merged result, as make targets.
const RUNS: [&str; 7] = [
    "qa",
    "smoke-producers",
    "smoke-charge",
    "smoke-lang",
    "doctest",
    "semver-check",
    "smoke-impl",
];

/// The targets that each take minutes, and so each take a job of their own.
const LONG: [&str; 5] = [
    "qa",
    "smoke-producers",
    "smoke-charge",
    "smoke-lang",
    "smoke-impl",
];

/// The queue's jobs but the gate, as (name, body).
fn queue_jobs() -> Vec<(String, String)> {
    let jobs: Vec<(String, String)> = workflow_jobs(&workflow(WORKFLOW))
        .into_iter()
        .filter(|(name, _)| name != GATE)
        .collect();
    assert!(
        !jobs.is_empty(),
        "found no jobs in {WORKFLOW}; the parse is broken, not the workflow"
    );
    jobs
}

/// The queue's gate job.
fn gate() -> String {
    workflow_jobs(&workflow(WORKFLOW))
        .into_iter()
        .find(|(name, _)| name == GATE)
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("{WORKFLOW} defines no `{GATE}` job"))
}

// A pass that runs twice costs a job its length again, and a pass that runs
// nowhere checks nothing on the merged result. The counter-factual: drop the
// charge job, and a merge that moves a file's charge past its row lands red.
#[test]
fn the_queue_runs_qa_and_each_pass_once() {
    let jobs = queue_jobs();
    for target in RUNS {
        let at: Vec<&String> = jobs
            .iter()
            .filter(|(_, body)| runs_target(body, target))
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            at.len(),
            1,
            "{WORKFLOW} runs `make {target}` in {} jobs: {at:?}",
            at.len()
        );
    }
}

// The queue costs its slowest job. The counter-factual: one job ran `make qa`
// and then every pass in series, and each merge waited about an hour for what
// five jobs side by side finish in the time of the slowest.
#[test]
fn no_queue_job_runs_two_long_targets() {
    for (name, body) in queue_jobs() {
        let long: Vec<&str> = LONG
            .into_iter()
            .filter(|target| runs_target(&body, target))
            .collect();
        assert!(
            long.len() <= 1,
            "{WORKFLOW} job `{name}` runs {long:?} in series, so the queue \
             costs their sum"
        );
    }
}

// A job that waits for another adds the other's length to every merge. The
// counter-factual: the suite jobs wait for QA, as they do on a pull request,
// and the queue costs QA plus its slowest pass.
#[test]
fn no_queue_job_waits_for_another() {
    for (name, body) in queue_jobs() {
        let needs = job_needs(&body);
        assert!(
            needs.is_empty(),
            "{WORKFLOW} job `{name}` waits for {needs:?}"
        );
    }
}

// Branch protection requires "All Checks Passed" alone, so the gate's list is
// what blocks a merge. The counter-factual: a job missing from `needs` runs and
// reports beside a merge it cannot block, and a job in `needs` with no
// `needs.NAME.result` step is waited for and then ignored.
#[test]
fn the_queue_gate_waits_for_and_fails_on_every_job() {
    let defined: BTreeSet<String> = queue_jobs().into_iter().map(|(name, _)| name).collect();
    let gate = gate();
    let needs: BTreeSet<String> = job_needs(&gate).into_iter().collect();
    assert_eq!(
        needs, defined,
        "`{GATE}` in {WORKFLOW} waits for {needs:?}, and the jobs are {defined:?}"
    );
    assert_eq!(
        enforced_results(&gate),
        defined,
        "`{GATE}` in {WORKFLOW} does not fail on the result of every job"
    );
}
