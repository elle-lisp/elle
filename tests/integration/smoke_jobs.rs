// audited: 2026-11-24
// Which pull-request job runs each pass of a build, and which binary each Smoke job builds.
//
// docs/analysis/ci.md
//
// A pass that no job runs fails nothing, and a job that builds a binary it
// never runs is only slower. So these tests read the jobs, and ask `make
// --dry-run` what each pass runs on and what each step builds.

use crate::common::{
    make_dry_run_with, make_expand, make_steps, runs_on, runs_target, smoke_passes, step_lines,
    suite_targets, the_pass, workflow_files, workflow_jobs,
};
use std::collections::BTreeSet;

/// The runners whose Smoke jobs run every pass of `make smoke`.
const SMOKE_RUNNERS: [&str; 2] = ["ubuntu-24.04-arm", "macos-latest"];

/// The runners where the default build's corpus splits into a job on `elle`,
/// a job on the rig, and a job for the charge pass.
const SPLIT_RUNNERS: [&str; 3] = ["ubuntu-latest", "ubuntu-24.04-arm", "macos-latest"];

/// The pull-request workflow's jobs, as (name, body).
fn pr_jobs() -> Vec<(String, String)> {
    let (_, text) = workflow_files()
        .into_iter()
        .find(|(path, _)| path == ".github/workflows/pr.yml")
        .expect(".github/workflows/pr.yml exists");
    workflow_jobs(&text)
}

/// The jobs whose `runs-on:` names `label`.
fn on<'a>(jobs: &'a [(String, String)], label: &str) -> Vec<&'a (String, String)> {
    jobs.iter()
        .filter(|(_, body)| runs_on(body) == Some(label))
        .collect()
}

/// The default build's suite passes that run on its rig.
fn rig_passes() -> Vec<String> {
    let rig = make_expand("ELLE_RIG");
    let suites = suite_targets();
    let mut targets = smoke_passes();
    targets.push("smoke-scrub".to_string());
    targets
        .into_iter()
        .filter(|target| suites.contains(target))
        .filter(|target| the_pass(target).runner() == rig)
        .collect()
}

// Each build's passes run on every runner that runs the build, each as a step,
// so dividing a platform's passes between two jobs dropped none of them. The
// counter-factual: the pool job ran `make smoke-pool` alone after the split of
// one target into one per pass, and the pool rig's implementation suite ran on
// no job.
#[test]
fn every_runner_that_runs_a_build_runs_every_pass_of_it() {
    let jobs = pr_jobs();
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
        let runners: BTreeSet<&str> = jobs
            .iter()
            .filter(|(_, body)| runs_target(body, lead))
            .filter_map(|(_, body)| runs_on(body))
            .collect();
        assert!(!runners.is_empty(), "no job runs `make {lead}`");
        for runner in runners {
            let here = on(&jobs, runner);
            for target in targets {
                assert!(
                    here.iter().any(|(_, body)| runs_target(body, target)),
                    "a job on {runner} runs `make {lead}`, and no job there runs \
                     `make {target}`"
                );
            }
        }
    }
}

// A platform's Smoke jobs run each pass of `make smoke` as a step of its own,
// and each once on the platform (docs/analysis/ci.md). The counter-factual: a
// `make doctest` step, then a `make smoke` step, runs the doctests twice, and a
// red `make smoke` step names no pass. A Smoke job runs no `make qa`, which is
// not a pass: the QA job is the QA gate.
#[test]
fn each_platforms_smoke_jobs_run_every_pass_of_make_smoke_once() {
    let jobs = pr_jobs();
    let passes = smoke_passes();
    assert!(
        passes.len() > 2,
        "`make smoke` names {passes:?}; the parse is broken, not the Makefile"
    );
    assert!(
        !passes.iter().any(|pass| pass == "qa"),
        "`SMOKE_PASSES` names `qa`, so every Smoke job would run QA again"
    );
    for runner in SMOKE_RUNNERS {
        let steps: Vec<&str> = on(&jobs, runner)
            .into_iter()
            .filter(|(_, body)| passes.iter().any(|pass| runs_target(body, pass)))
            .flat_map(|(_, body)| step_lines(body))
            .collect();
        assert!(
            !steps.is_empty(),
            "no job on {runner} runs a pass of `make smoke`"
        );
        assert!(
            !steps.iter().any(|line| runs_target(line, "smoke")),
            "a job on {runner} runs `make smoke` whole, so a red step names no pass"
        );
        for pass in &passes {
            let n = steps.iter().filter(|line| runs_target(line, pass)).count();
            assert_eq!(
                n, 1,
                "the Smoke jobs on {runner} run `make {pass}` {n} times"
            );
        }
    }
}

// On every runner of the default build the language suite runs on `elle` in
// one job, and the rig's passes run in others at the same time
// (docs/analysis/ci.md). The counter-factual: one Smoke job ran every pass in
// series, so the platform cost the sum of the two halves, and the macOS one set
// the wall clock of every pull request. AArch64 kept that one job after the
// split, and became the second-slowest job of the workflow.
#[test]
fn the_language_suite_and_the_rig_run_in_separate_jobs() {
    let jobs = pr_jobs();
    let rig = rig_passes();
    assert!(
        rig.len() >= 4,
        "found the rig passes {rig:?}; the parse is broken, not the Makefile"
    );
    for runner in SPLIT_RUNNERS {
        let here = on(&jobs, runner);
        let names = |keep: &dyn Fn(&str) -> bool| -> BTreeSet<String> {
            here.iter()
                .filter(|(_, body)| keep(body))
                .map(|(name, _)| name.clone())
                .collect()
        };
        let lang = names(&|body| runs_target(body, "smoke-lang"));
        let rigs = names(&|body| rig.iter().any(|target| runs_target(body, target)));
        assert!(
            !lang.is_empty(),
            "no job on {runner} runs `make smoke-lang`"
        );
        assert!(
            !rigs.is_empty(),
            "no job on {runner} runs a pass on the rig"
        );
        let both: Vec<&String> = lang.intersection(&rigs).collect();
        assert!(
            both.is_empty(),
            "these jobs on {runner} run the language suite and the rig's passes \
             in series: {both:?}"
        );
    }
}

// The job that runs the language suite on `elle` runs nothing on the rig, so it
// builds no rig. The trap: a target builds what its prerequisites build, so a
// step named for `elle` alone can build the rig too, and only the dry run shows
// it. The counter-factual: the job keeps a `make elle-rig` step and pays for a
// binary it never runs.
#[test]
fn the_language_suite_job_builds_no_rig() {
    let jobs = pr_jobs();
    let mut checked = 0;
    for runner in SPLIT_RUNNERS {
        for (name, body) in on(&jobs, runner) {
            if !runs_target(body, "smoke-lang") {
                continue;
            }
            checked += 1;
            for (target, vars) in make_steps(body) {
                let vars: Vec<&str> = vars.iter().map(String::as_str).collect();
                let recipe = make_dry_run_with(&target, &vars)
                    .unwrap_or_else(|| panic!("`make --dry-run {target}` did not run"));
                assert!(
                    !recipe.contains("-p elle-rig"),
                    "job `{name}` on {runner} runs `make {target}`, which builds \
                     the rig the job never runs"
                );
            }
        }
    }
    assert_eq!(
        checked,
        SPLIT_RUNNERS.len(),
        "found {checked} jobs running `make smoke-lang` on {SPLIT_RUNNERS:?}"
    );
}

// The charge pass is the longest pass on every runner, so it runs in a job that
// runs no other suite pass (docs/analysis/ci.md). The counter-factual: the rig
// job ran it after three other passes, and the macOS rig job set the wall clock
// of every pull request at about 50 minutes.
#[test]
fn the_charge_pass_runs_in_a_job_of_its_own() {
    let jobs = pr_jobs();
    let suites = suite_targets();
    for runner in SPLIT_RUNNERS {
        let charge: Vec<&(String, String)> = on(&jobs, runner)
            .into_iter()
            .filter(|(_, body)| runs_target(body, "smoke-charge"))
            .collect();
        assert_eq!(
            charge.len(),
            1,
            "{} jobs on {runner} run `make smoke-charge`",
            charge.len()
        );
        let (name, body) = charge[0];
        let others: Vec<String> = make_steps(body)
            .into_iter()
            .map(|(target, _)| target)
            .filter(|target| target != "smoke-charge" && suites.contains(target))
            .collect();
        assert!(
            others.is_empty(),
            "job `{name}` on {runner} runs the charge pass after {others:?}, so the \
             platform waits for their sum"
        );
    }
}
