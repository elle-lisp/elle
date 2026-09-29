// audited: 2026-09-29
// Every corpus target runs the leak dashboards through `elle test --isolate`, so their verdicts are rows.
//
// docs/testing.md
// docs/test-store.md
//
// The counter-factual: the targets ran `timeout 120s elle --jit=off
// tests/elle/oracle.lisp` beside the runner. The dashboard passed or failed the
// gate, and every rate it measured scrolled past in the log — the measurement
// channel is opened only for an isolated child, so no row was ever written.

use crate::common::{budget_seconds, make_dry_run, make_expand, makefile};

/// The two dashboards, each with the Makefile variable holding its budget.
const DASHBOARDS: &[(&str, &str)] = &[("ORACLE_FILE", "ORACLE_TIMEOUT"), ("PLUMB_FILE", "PLUMB_TIMEOUT")];

/// The JIT policies the old direct runs covered, and the runner's children
/// must still cover.
const POLICIES: &[&str] = &["--jit=off", "--jit=eager"];

/// Every target whose recipe runs the corpus through the runner. Read from the
/// Makefile rather than listed here, so a target added later is held to the
/// same rule without an edit to this file.
fn runner_targets() -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in makefile().lines() {
        if line.starts_with('\t') {
            if line.contains("$(RUN_CORPUS)") {
                if let Some(t) = &current {
                    if !out.contains(t) {
                        out.push(t.clone());
                    }
                }
            }
            continue;
        }
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with(' ') {
            continue;
        }
        current = line
            .split_once(':')
            .map(|(head, _)| head)
            .filter(|head| !head.is_empty() && !head.contains('=') && !head.contains(' '))
            .map(str::to_string);
    }
    assert!(
        out.len() >= 3,
        "found {} targets running RUN_CORPUS ({out:?}); the scan is broken, not the Makefile",
        out.len()
    );
    out
}

/// The lines of `make --dry-run TARGET` that name `file`.
fn lines_naming(target: &str, file: &str) -> Vec<String> {
    let recipe = make_dry_run(target).unwrap_or_else(|| panic!("`make --dry-run {target}` failed"));
    recipe
        .lines()
        .filter(|line| line.contains(file))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_runner_target_records_each_dashboard_under_both_policies() {
    let elle = make_expand("ELLE");
    let runner = format!("{elle} test");
    for target in runner_targets() {
        for (file_var, _) in DASHBOARDS {
            let file = make_expand(file_var);
            let lines = lines_naming(&target, &file);
            for policy in POLICIES {
                assert!(
                    lines.iter().any(|l| l.contains(&runner)
                        && l.contains("--isolate")
                        && l.contains(policy)),
                    "`make {target}` does not run {file} through `{runner} --isolate` \
                     under {policy}, so its verdicts reach no measurement row:\n{}",
                    lines.join("\n")
                );
            }
            for line in &lines {
                assert!(
                    line.contains(&runner),
                    "`make {target}` still runs {file} outside the runner:\n  {line}"
                );
            }
        }
    }
}

// A dashboard's budget is its own. The runner's default is 60 s for every
// path, and the oracle takes most of two minutes on a CI runner: a run that
// dropped the budget would record the oracle as a timeout on every pass.
#[test]
fn each_dashboard_takes_its_own_budget() {
    for (file_var, budget_var) in DASHBOARDS {
        let file = make_expand(file_var);
        let ms = budget_seconds(&make_expand(budget_var)) * 1000;
        for line in lines_naming("smoke-elle", &file) {
            assert!(
                line.contains(&format!("--timeout {ms}")),
                "{file} runs without its {budget_var} budget ({ms} ms):\n  {line}"
            );
        }
    }
}
