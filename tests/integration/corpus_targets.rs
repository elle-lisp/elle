// audited: 2026-10-06
// The suite targets run the binary through the runner alone, `make qa` runs what the QA job runs, and runs first.
//
// docs/testing.md
// docs/analysis/testing.md
//
// Every assertion reads `make --dry-run`, which prints each command a target
// will run with every variable resolved. A target that runs a suite file
// outside the runner still passes or fails the gate, but its verdict is a line
// in a CI log rather than a row in the session DB, so the recipe is checked
// here.

use crate::common::{make_dry_run, make_expand, workflow_files, workflow_jobs};

/// What `make --dry-run TARGET` would run.
fn recipe(target: &str) -> String {
    make_dry_run(target).unwrap_or_else(|| panic!("`make --dry-run {target}` failed"))
}

/// Whether `line` runs `program`: the program's path as a word of the line.
fn runs(line: &str, program: &str) -> bool {
    line.split_whitespace().any(|word| word == program)
}

// The counter-factual: a target that runs a per-file pass beside the runner,
// one `elle FILE` per file under `parallel` and `timeout`. Every file still
// gates, and nothing it finds reaches the session DB. `smoke-wasm` ran its
// language suite that way.
#[test]
fn every_suite_target_runs_the_binary_through_the_runner_alone() {
    let programs: Vec<String> = ["ELLE", "ELLE_WASM", "ELLE_MLIR"]
        .into_iter()
        .map(make_expand)
        .collect();
    let rigs: Vec<String> = ["ELLE_RIG", "ELLE_RIG_WASM", "ELLE_RIG_MLIR"]
        .into_iter()
        .map(make_expand)
        .collect();
    for (target, runner) in [
        ("smoke-lang", "ELLE"),
        ("smoke-impl", "ELLE_RIG"),
        ("smoke-nojit", "ELLE"),
        ("smoke-pool", "ELLE"),
        ("smoke-mlir", "ELLE_MLIR"),
        ("smoke-noffi", "ELLE"),
        ("smoke-wasm", "ELLE_WASM"),
    ] {
        let recipe = recipe(target);
        let elle = make_expand(runner);
        let runs_test: Vec<&str> = recipe
            .lines()
            .filter(|line| line.contains(&format!("{elle} test")))
            .collect();
        assert!(
            !runs_test.is_empty(),
            "`make {target}` never runs `{elle} test`:\n{recipe}"
        );
        for line in recipe.lines() {
            for program in &programs {
                assert!(
                    !runs(line, program) || line.contains(&format!("{program} test")),
                    "`make {target}` runs {program} outside the runner, so what \
                     it finds reaches no session DB:\n  {line}"
                );
            }
            for rig in &rigs {
                assert!(
                    !runs(line, rig)
                        || line.contains("--host")
                        || line.contains(&format!("{rig} test")),
                    "`make {target}` runs the rig outside the runner:\n  {line}"
                );
            }
            assert!(
                !runs(line, "parallel"),
                "`make {target}` hands files to `parallel`, one process per file \
                 outside the runner:\n  {line}"
            );
        }
    }
}

/// Assert that `make TARGET` runs `qa`'s `cargo fmt --check` before its first
/// runner line.
fn assert_qa_before_the_suites(target: &str) {
    let elle = make_expand("ELLE");
    let recipe = recipe(target);
    let first = |what: &str, found: &dyn Fn(&str) -> bool| {
        recipe
            .lines()
            .position(found)
            .unwrap_or_else(|| panic!("`make {target}` never runs {what}:\n{recipe}"))
    };
    let fmt = first("`cargo fmt --check`", &|line| {
        line.contains("cargo fmt --check")
    });
    let runner = first("the runner", &|line| line.contains(&format!("{elle} test")));
    assert!(
        fmt < runner,
        "`make {target}` runs the suites before `qa`: `cargo fmt --check` is \
         line {fmt}, the first runner line {runner}"
    );
}

// `qa` takes about two minutes and the suites about thirty. The
// counter-factual: `test` lists `smoke` first, so a formatting or clippy
// failure surfaces half an hour into the run, after suites it has nothing to
// do with.
#[test]
fn make_test_runs_qa_before_the_suites() {
    assert_qa_before_the_suites("test");
}

// `make smoke` is what a contributor runs before a push and what the merge
// queue runs. The counter-factual: a `smoke` without `qa` passes after thirty
// minutes, the pull request's QA job fails on a clippy warning, and the fix
// costs a second smoke.
#[test]
fn make_smoke_runs_qa_before_the_suites() {
    assert_qa_before_the_suites("smoke");
}

// `make qa` is the QA job, locally, so every make target the job runs is a
// target `qa` runs. The counter-factual: the job ran `make agents-check` and
// `qa` did not, so a stale generated index passed every local gate and failed
// the pull request.
#[test]
fn make_qa_runs_every_make_target_the_qa_job_runs() {
    let text = workflow_files()
        .into_iter()
        .find(|(path, _)| path.ends_with("/pr.yml"))
        .map(|(_, text)| text)
        .expect("the pull-request workflow exists");
    let body = workflow_jobs(&text)
        .into_iter()
        .find(|(name, _)| name == "qa")
        .map(|(_, body)| body)
        .expect("the pull-request workflow defines a `qa` job");
    let targets: Vec<&str> = body
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: make "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    assert!(
        !targets.is_empty(),
        "the `qa` job runs no make target; the parse is broken, not the workflow"
    );
    let qa = recipe("qa");
    for target in targets {
        for line in recipe(target).lines() {
            assert!(
                qa.lines().any(|l| l == line),
                "the `qa` job runs `make {target}`, and `make qa` never runs:\n  {line}"
            );
        }
    }
}
