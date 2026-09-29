// audited: 2026-09-29
// How the corpus targets run the corpus, and when `make test` reaches it, as read from what `make` would run.
//
// docs/testing.md
// docs/analysis/ci.md
// docs/analysis/testing.md
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

// The counter-factual: `smoke-nouring` ran two per-file passes under
// `--no-uring`, one process per file, and every pool-only failure it found was
// a line in a CI log.
#[test]
fn smoke_nouring_runs_the_corpus_through_the_runner_alone() {
    runs_the_corpus_through_the_runner_alone("smoke-nouring");
}

// `qa` takes about two minutes and the corpus about thirty. The counter-factual:
// `test` listed `smoke` first, so a formatting or clippy failure surfaced half
// an hour into the run, after the corpus it had nothing to do with.
#[test]
fn make_test_runs_qa_before_the_corpus() {
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
    let per_file = first("a per-file pass", &|line| line.contains("parallel "));
    assert!(
        fmt < runner && fmt < per_file,
        "`make test` runs the corpus before `qa`: `cargo fmt --check` is line {fmt}, \
         the runner line {runner}, the first per-file pass line {per_file}"
    );
}

// The pool is a build, not a flag. A target that ran the default binary would
// run the corpus on the ring — green, and saying nothing about the backend a
// Mac runs.
#[test]
fn smoke_nouring_builds_the_no_uring_feature() {
    let recipe = recipe("smoke-nouring");
    assert!(
        recipe
            .lines()
            .any(|line| line.contains("cargo build") && line.contains("--features no-uring")),
        "`make smoke-nouring` builds no binary with the no-uring feature:\n{recipe}"
    );
}
