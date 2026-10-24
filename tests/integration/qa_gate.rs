// audited: 2026-10-23
// QA runs on every pull request, runs the prose checks first, judges the tree's
// ledgers, and every other job waits for it.
//
// docs/analysis/ci.md
//
// QA fails in minutes, and the rest of the matrix takes up to an hour. A job
// that starts beside QA spends that hour on a change QA has already refused.

use crate::common::{
    assert_rig_runs, make_dry_run, repo_root, the_pass, workflow_jobs as jobs, TREE_PRODUCERS,
};
use std::collections::BTreeSet;

/// The QA job, as `.github/workflows/pr.yml` names it.
const QA: &str = "qa";

fn workflow_text() -> String {
    let path = repo_root().join(".github/workflows/pr.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The body of the QA job.
fn qa_body(text: &str) -> String {
    jobs(text)
        .into_iter()
        .find(|(name, _)| name == QA)
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("the pull-request workflow defines no `{QA}` job"))
}

/// The jobs one job's `needs` names, written as one name or as a bracketed
/// list. Only the job's own key counts, at four spaces of indent.
fn needs(body: &str) -> Vec<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("    needs:"))
        .map(|rest| {
            rest.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The job's own `if:` condition, at four spaces of indent.
fn condition(body: &str) -> Option<&str> {
    body.lines()
        .find_map(|line| line.strip_prefix("    if:"))
        .map(str::trim)
}

// The counter-factual: the Rust Tests jobs started beside QA, and an over-long
// call-out failed all three of them 20 to 47 minutes in. `changes` reads only
// the diff, so it starts beside QA. The gate job names `qa` like every other.
#[test]
fn every_job_waits_for_qa() {
    let text = workflow_text();
    let all = jobs(&text);
    assert!(
        all.len() > 5,
        "found only {} jobs; the parse is broken, not the workflow",
        all.len()
    );
    let starting: Vec<&String> = all
        .iter()
        .filter(|(name, _)| name != QA && name != "changes")
        .filter(|(_, body)| !needs(body).iter().any(|need| need == QA))
        .map(|(name, _)| name)
        .collect();
    assert!(
        starting.is_empty(),
        "these jobs start without waiting for `{QA}`, so they run on a change \
         QA refuses: {starting:?}"
    );
}

// A status function in a job's condition runs the job whatever its `needs`
// answered, so a job that waits for QA and says `always()` still runs after QA
// fails. Only the gate job may say so: it has to run to report the failure.
#[test]
fn no_job_but_the_gate_runs_after_qa_fails() {
    let text = workflow_text();
    let overriding: Vec<String> = jobs(&text)
        .into_iter()
        .filter(|(name, _)| name != "all-checks")
        .filter(|(_, body)| {
            condition(body).is_some_and(|c| {
                ["always()", "failure()", "cancelled()"]
                    .iter()
                    .any(|f| c.contains(f))
            })
        })
        .map(|(name, _)| name)
        .collect();
    assert!(
        overriding.is_empty(),
        "these jobs run whatever `{QA}` answered: {overriding:?}"
    );
}

// The prose checks read every stamped file, documents included. The
// counter-factual: QA carried the `source` gate, so a pull request that touched
// only Markdown ran no prose check at all.
#[test]
fn qa_runs_on_every_pull_request() {
    let text = workflow_text();
    let body = qa_body(&text);
    assert_eq!(
        condition(&body),
        None,
        "the `{QA}` job runs only under a condition, so a pull request outside \
         it merges without QA"
    );
}

/// The command each step of a job runs, in order.
fn runs(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            line.trim_start()
                .trim_start_matches("- ")
                .strip_prefix("run:")
        })
        .map(|rest| rest.trim().to_string())
        .collect()
}

// The prose checks are the cheapest check in QA, and the one an agent's change
// fails most often. The counter-factual: QA ran clippy and rustdoc first, so a
// sentence one word too long waited minutes behind them to fail.
#[test]
fn the_qa_job_runs_the_prose_checks_first() {
    let text = workflow_text();
    let steps = runs(&qa_body(&text));
    assert_eq!(
        steps.first().map(String::as_str),
        Some("make prose"),
        "the `{QA}` job's first step is not `make prose`: {steps:?}"
    );
}

// `make qa` is the QA job, locally, so it runs the prose checks first too. The
// counter-factual: `make qa` ran clippy first, and a local run took minutes to
// report what the prose checks report in seconds.
#[test]
fn make_qa_runs_the_prose_checks_first() {
    let recipe = make_dry_run("qa").expect("`make --dry-run qa` succeeds");
    let first = recipe
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    assert!(
        first.starts_with("cargo test --test prose"),
        "`make qa` does not run the prose checks first; its first command is:\n  {first}"
    );
}

// The tree's producers read the tree and not the build, so `make
// tree-producers` judges them on the release rig, in-process and in a pass of
// their own, under both JIT policies (docs/analysis/ci.md). The counter-factual:
// they rode the default build's producer pass, its seventh step, and a stamp
// that moved an audit count failed there ten minutes after QA had passed.
#[test]
fn make_tree_producers_judges_the_tree_ledgers_on_the_release_rig() {
    let pass = the_pass("tree-producers");
    let want: BTreeSet<String> = TREE_PRODUCERS.iter().map(|p| p.to_string()).collect();
    assert_eq!(
        pass.files, want,
        "`make tree-producers` runs the tree's producers alone"
    );
    assert_eq!(pass.isolate(), None, "and runs them in-process");
    assert_rig_runs(&pass, "RELEASE_RIG", "the tree's producer pass");
    let recipe = make_dry_run("tree-producers").expect("`make --dry-run tree-producers` succeeds");
    assert!(
        recipe.contains("cargo build --release -p elle-rig"),
        "`make tree-producers` runs a release rig it never builds:\n{recipe}"
    );
}

// QA is the gate every other job waits for, so the tree's ledgers fail it, and
// `make qa` runs what the job runs. The counter-factual: the job judged neither
// ledger, and every suite job started over a stale row.
#[test]
fn the_qa_job_judges_the_tree_ledgers() {
    let text = workflow_text();
    let steps = runs(&qa_body(&text));
    assert!(
        steps.iter().any(|s| s == "make tree-producers"),
        "the `{QA}` job never runs `make tree-producers`: {steps:?}"
    );
}
