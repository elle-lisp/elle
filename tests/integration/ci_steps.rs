// audited: 2026-10-21
// A CI step runs one make target, and a make target runs at most one `elle test` pass.
//
// docs/analysis/ci.md
//
// GitHub times a step and folds its log, so a step is the unit a reader finds
// a slow pass and the end of one pass by. These tests read every workflow's run
// lines, and ask `make --dry-run` what each suite target runs.

use crate::common::{
    make_steps, passes, recording_targets, runs_target, smoke_passes, step_lines, workflow_files,
    workflow_jobs as jobs,
};
use std::collections::BTreeSet;

/// The targets that run one build's producer pass.
const PRODUCER_PASSES: [&str; 3] = [
    "smoke-producers",
    "smoke-pool-producers",
    "smoke-mlir-producers",
];

/// The pull-request workflow's text.
fn pr_workflow() -> String {
    workflow(".github/workflows/pr.yml")
}

/// One workflow's text, by its repo-relative path.
fn workflow(path: &str) -> String {
    workflow_files()
        .into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("{path} does not exist"))
}

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

// Each build's job runs every suite target of that build, each as a step, so
// splitting a target into one per pass dropped none of them from CI. The
// counter-factual: the pool job ran `make smoke-pool` alone after the split,
// and the pool rig's implementation suite ran on no job.
#[test]
fn each_build_job_runs_every_pass_of_its_build() {
    let text = pr_workflow();
    let all = jobs(&text);
    for targets in [
        &[
            "smoke-lang",
            "smoke-impl",
            "smoke-eager",
            "smoke-producers",
            "smoke-charge",
        ][..],
        &["smoke-pool", "smoke-pool-impl", "smoke-pool-producers"][..],
        &["smoke-mlir", "smoke-mlir-impl", "smoke-mlir-producers"][..],
    ] {
        let lead = targets[0];
        let owners: Vec<&(String, String)> = all
            .iter()
            .filter(|(_, body)| runs_target(body, lead))
            .collect();
        assert!(!owners.is_empty(), "no job runs `make {lead}`");
        for (name, body) in owners {
            for target in targets {
                assert!(
                    runs_target(body, target),
                    "job `{name}` runs `make {lead}` and not `make {target}`"
                );
            }
        }
    }
}

// The merge queue runs `make smoke` as steps: `make qa`, then each pass of
// `SMOKE_PASSES` once (docs/analysis/ci.md). The counter-factual: a single
// `make smoke` step names no pass when it goes red, and holds every batch
// summary of every pass in one log.
#[test]
fn the_merge_queue_runs_qa_then_every_pass_of_make_smoke_once() {
    let text = workflow(".github/workflows/merge-queue.yml");
    let body = jobs(&text)
        .into_iter()
        .find(|(name, _)| name == "tests")
        .map(|(_, body)| body)
        .expect("the merge queue defines a `tests` job");
    let steps = step_lines(&body);
    assert!(
        !steps.iter().any(|line| runs_target(line, "smoke")),
        "the merge queue runs `make smoke` whole, so a red step names no pass"
    );
    let qa = steps.iter().position(|line| runs_target(line, "qa"));
    assert!(qa.is_some(), "the merge queue runs no `make qa`");
    for pass in smoke_passes() {
        let at: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, line)| runs_target(line, &pass))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            at.len(),
            1,
            "the merge queue runs `make {pass}` {} times",
            at.len()
        );
        assert!(
            qa < Some(at[0]),
            "the merge queue runs `make {pass}` before `make qa`"
        );
    }
}

// A Smoke job runs each pass of `make smoke` as a step of its own, and each
// once (docs/analysis/ci.md). The counter-factual: a `make doctest` step, then
// a `make smoke` step, runs the doctests twice, and a red `make smoke` step
// names no pass. A Smoke job runs no `make qa`, which is not a pass: the QA
// job is the QA gate.
#[test]
fn each_platform_smoke_job_runs_every_pass_of_make_smoke_once() {
    let text = pr_workflow();
    let passes = smoke_passes();
    assert!(
        passes.len() > 2,
        "`make smoke` names {passes:?}; the parse is broken, not the Makefile"
    );
    for job in ["aarch64", "macos"] {
        let body = jobs(&text)
            .into_iter()
            .find(|(name, _)| name == job)
            .map(|(_, body)| body)
            .unwrap_or_else(|| panic!("pr.yml defines no `{job}` job"));
        let steps = step_lines(&body);
        assert!(
            !steps.iter().any(|line| runs_target(line, "smoke")),
            "job `{job}` runs `make smoke` whole, so a red step names no pass"
        );
        assert!(
            !passes.iter().any(|pass| pass == "qa"),
            "`SMOKE_PASSES` names `qa`, so every Smoke job would run QA again"
        );
        for pass in &passes {
            let n = steps.iter().filter(|line| runs_target(line, pass)).count();
            assert_eq!(n, 1, "job `{job}` runs `make {pass}` {n} times");
        }
    }
}

// A job runs its build's producer pass right after the build, before every
// other pass and the doctests: the pass is short, and a stale ledger row fails
// it as surely as a broken file fails a suite (docs/analysis/ci.md). `qa` and
// the tree's producers come before it in the merge queue, and both are QA.
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
