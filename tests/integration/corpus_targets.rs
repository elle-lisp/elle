// audited: 2026-09-29
// The suite targets run the binary through the runner alone, and `make test` reaches them after `qa`.
//
// docs/testing.md
// docs/analysis/testing.md
//
// Every assertion reads `make --dry-run`, which prints each command a target
// will run with every variable resolved. A target that runs a suite file
// outside the runner still passes or fails the gate, but its verdict is a line
// in a CI log rather than a row in the session DB, so the recipe is checked
// here.

use crate::common::{make_dry_run, make_expand};

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
// gates, and nothing it finds reaches the session DB.
#[test]
fn every_suite_target_runs_the_binary_through_the_runner_alone() {
    let elle = make_expand("ELLE");
    let rig = make_expand("ELLE_RIG");
    for target in [
        "smoke-lang",
        "smoke-impl",
        "smoke-nojit",
        "smoke-pool",
        "smoke-mlir",
        "smoke-noffi",
    ] {
        let recipe = recipe(target);
        let runner: Vec<&str> = recipe
            .lines()
            .filter(|line| line.contains(&format!("{elle} test")))
            .collect();
        assert!(
            !runner.is_empty(),
            "`make {target}` never runs `{elle} test`:\n{recipe}"
        );
        for line in recipe.lines() {
            assert!(
                !runs(line, &elle) || line.contains(&format!("{elle} test")),
                "`make {target}` runs the binary outside the runner, so what it \
                 finds reaches no session DB:\n  {line}"
            );
            assert!(
                !runs(line, &rig) || line.contains("--host"),
                "`make {target}` runs the rig outside the runner:\n  {line}"
            );
            assert!(
                !runs(line, "parallel"),
                "`make {target}` hands files to `parallel`, one process per file \
                 outside the runner:\n  {line}"
            );
        }
    }
}

// `qa` takes about two minutes and the suites about thirty. The
// counter-factual: `test` lists `smoke` first, so a formatting or clippy
// failure surfaces half an hour into the run, after suites it has nothing to
// do with.
#[test]
fn make_test_runs_qa_before_the_suites() {
    let elle = make_expand("ELLE");
    let recipe = recipe("test");
    let first = |what: &str, found: &dyn Fn(&str) -> bool| {
        recipe
            .lines()
            .position(found)
            .unwrap_or_else(|| panic!("`make test` never runs {what}:\n{recipe}"))
    };
    let fmt = first("`cargo fmt --check`", &|line| line.contains("cargo fmt --check"));
    let runner = first("the runner", &|line| line.contains(&format!("{elle} test")));
    assert!(
        fmt < runner,
        "`make test` runs the suites before `qa`: `cargo fmt --check` is line \
         {fmt}, the first runner line {runner}"
    );
}
