// audited: 2026-09-29
// Which corpus targets run the corpus through `elle test` alone, as read from what `make` would run.
//
// docs/testing.md
// docs/analysis/ci.md
//
// A target that runs the corpus one process per file, beside or instead of the
// runner, records nothing: every verdict it reaches is a line in a CI log. The
// targets named here run the corpus through the runner and nothing else, so
// each of their verdicts is a row in the session DB.

use crate::common::{make_dry_run, make_expand};

/// What `make --dry-run TARGET` would run.
fn recipe(target: &str) -> String {
    make_dry_run(target).unwrap_or_else(|| panic!("`make --dry-run {target}` failed"))
}

/// Assert that every run of the binary in TARGET's recipe is a runner run, and
/// that no line hands the corpus to `parallel`.
fn runs_the_corpus_through_the_runner_alone(target: &str) {
    let elle = make_expand("ELLE");
    let recipe = recipe(target);
    let runs: Vec<&str> = recipe
        .lines()
        .filter(|line| line.contains(&format!("{elle} ")))
        .collect();
    assert!(
        runs.iter().any(|line| line.contains(&format!("{elle} test"))),
        "`make {target}` never runs `{elle} test`:\n{recipe}"
    );
    for line in &runs {
        assert!(
            line.contains(&format!("{elle} test")),
            "`make {target}` runs the binary outside the runner, so what it finds \
             reaches no session DB:\n  {line}"
        );
    }
    for line in recipe.lines() {
        assert!(
            !line.contains("parallel "),
            "`make {target}` hands files to `parallel`, one process per file:\n  {line}"
        );
    }
}

// The counter-factual: `smoke-mlir` ran a whole-file `--mlir=eager` pass after
// the runner, one process per file, and the oracle and plumb directly under it.
// The runner already puts every single-form file on the mlir-cpu tier.
#[test]
fn smoke_mlir_runs_the_corpus_through_the_runner_alone() {
    runs_the_corpus_through_the_runner_alone("smoke-mlir");
}
