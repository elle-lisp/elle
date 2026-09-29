// audited: 2026-09-29
// The wall-clock budget `elle test` gives one form, and how the suite passes
// tell it which paths need a wider one.
//
// A suite pass hands one runner process a whole batch of files, so no shell
// sees a path in time to choose a budget for it. The policy travels to the
// runner as flags instead, and this file checks that it arrives and that the
// runner acts on it.
//
// Three places have to agree, and nothing compiles any of them. The Makefile
// names the families. The recipe passes them. The runner reads them when it
// sets a form's deadline. Every pair of those can be right while the third is
// wrong, and the failure is the same either way: a file is killed at a budget
// narrower than its own deadline, so the diagnostic it exists to print never
// prints and CI reports a bare `timeout`.
//
// docs/test-cli.md

use crate::common::{
    declared_deadline, make_dry_run, make_expand as expand, repo_root, suite_files,
    wide_patterns,
};
use std::collections::BTreeMap;
use std::process::Command;

/// The flags every suite pass hands `elle test`, ready for a `Command`.
fn wide_flags() -> Vec<String> {
    let flags = expand("WIDE_FLAGS");
    let words: Vec<String> = flags.split_whitespace().map(str::to_string).collect();
    assert!(
        !words.is_empty(),
        "WIDE_FLAGS expands to nothing, so a suite pass tells the runner no policy"
    );
    words
}

/// The wide budget in milliseconds, which is what the runner takes.
fn wide_ms() -> u64 {
    expand("WIDE_TIMEOUT_MS")
        .parse()
        .unwrap_or_else(|_| panic!("WIDE_TIMEOUT_MS is not a whole number of milliseconds"))
}

/// The per-form budget the runner gives each of `paths`, in milliseconds, read
/// from the runner itself under `flags`.
///
/// `elle test --budget` prints a budget per path and exits without touching the
/// session store. Asking the binary is the whole point: the selector belongs to
/// the thing under test, and a copy of its rule written here is a second runner
/// that can disagree with the first.
fn runner_budgets(flags: &[String], paths: &[String]) -> BTreeMap<String, u64> {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .current_dir(repo_root())
        .arg("test")
        .arg("--budget")
        .args(flags)
        .args(paths)
        .output()
        .expect("run `elle test --budget`");
    assert!(
        out.status.success(),
        "`elle test --budget` failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("the runner prints budgets");
    let mut budgets = BTreeMap::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(ms), Some(path)) = (words.next(), words.next()) else {
            continue;
        };
        let ms = ms
            .parse()
            .unwrap_or_else(|_| panic!("a budget is a whole number of milliseconds: {line}"));
        budgets.insert(path.to_string(), ms);
    }
    assert_eq!(
        budgets.len(),
        paths.len(),
        "`elle test --budget` answered for {} of {} paths:\n{text}",
        budgets.len(),
        paths.len()
    );
    budgets
}

/// The budget the runner gives a path it was told nothing about — its own
/// `--timeout` default. Read rather than written down: the default is the
/// runner's to choose, and a number here would only record what it once was.
fn runner_default_ms() -> u64 {
    let path = "tests/lang/arithmetic.lisp".to_string();
    runner_budgets(&[], std::slice::from_ref(&path))[&path]
}

/// Every `elle test` batch line `make TARGET` will run.
///
/// `--dry-run` rather than the Makefile's text: a flag and its value meet on a
/// recipe line through several variables, and what the pass runs is the only
/// thing worth asserting on. A check that greps the source for `$(WIDE_FLAGS)`
/// passes on a variable that expands to nothing.
fn batch_lines(target: &str) -> Vec<String> {
    let recipe =
        make_dry_run(target).unwrap_or_else(|| panic!("`make --dry-run {target}` did not run"));
    let lines: Vec<String> = recipe
        .lines()
        .filter(|line| line.contains(" test ") && line.contains("xargs"))
        .map(str::to_string)
        .collect();
    assert!(
        !lines.is_empty(),
        "`make {target}` runs no `elle test` batch:\n{recipe}"
    );
    lines
}

// A policy the recipe never passes is a policy the runner never hears. Every
// other check here drives the runner directly, so every other check can pass
// while a suite still runs each file under the default.
//
// The counter-factual: teach the runner `--wide`, cover it, and leave a suite
// pass spelling `elle test` without it.
//
// A batch line must also carry no bare `--timeout`. One wide `--timeout` for
// the whole batch clears every declared deadline and reads as a fix — and it
// hands the wide budget to every ordinary file too, so a hang the gate used to
// catch in a minute now costs two and a half.
#[test]
fn every_suite_pass_hands_the_runner_the_wide_policy() {
    let flags = wide_flags().join(" ");
    for target in ["smoke-lang", "smoke-impl"] {
        for batch in batch_lines(target) {
            assert!(
                batch.contains(&flags),
                "`make {target}` runs the runner without the wide-family policy:\n  {}\n\
                 Every file in the batch then takes the runner's default budget, \
                 including the families whose own deadline is wider than it.\n\
                 Expected the line to carry: {flags}",
                batch.trim()
            );
            assert!(
                !batch.contains("--timeout"),
                "`make {target}` widens every file in the batch with one `--timeout`:\n  {}\n\
                 The named families need the wider budget; every other file needs \
                 the default, which is what makes a hang fail fast.",
                batch.trim()
            );
        }
    }
}

// A family that matches nothing is not an error anywhere: the runner simply
// never widens it, and the file it was written for — under whatever name it
// now has — goes back to the default budget and starts timing out under load.
// The counter-factual: rename a heavy file without touching the Makefile, and
// every gate still passes until a runner is slow enough.
#[test]
fn every_family_named_for_the_wider_budget_matches_a_suite_file() {
    let files = suite_files();
    for pattern in wide_patterns() {
        assert!(
            files.iter().any(|path| path.contains(&pattern)),
            "the Makefile gives `{pattern}` the wider budget, but no suite file \
             matches it, so whatever those files are called now run under the \
             runner's default."
        );
    }
}

// The runner's half, driven through the runner. A flag it accepts and ignores
// passes every check above — the Makefile names the families, the recipe passes
// them, and every form still runs under the default. Only the runner can say
// what a path's budget is.
//
// The counter-factual: parse the flags and never consult them where the
// deadline is chosen. Nothing else in this file notices.
#[test]
fn the_runner_widens_the_named_families_and_nothing_else() {
    let default_ms = runner_default_ms();
    assert!(
        wide_ms() > default_ms,
        "the wide budget is {} ms and the runner's default is {default_ms} ms, \
         so widening takes budget away",
        wide_ms()
    );

    // Both controls have to exist, or the narrow arm is measured against a path
    // the suite never runs.
    let controls = ["tests/lang/arithmetic.lisp", "tests/lang/strings.lisp"];
    for path in controls {
        assert!(repo_root().join(path).exists(), "{path} is gone");
    }

    let mut paths: Vec<String> = controls.iter().map(|p| (*p).to_string()).collect();
    for pattern in wide_patterns() {
        paths.extend(
            suite_files()
                .into_iter()
                .filter(|path| path.contains(&pattern)),
        );
    }
    paths.sort();
    paths.dedup();

    for (path, ms) in runner_budgets(&wide_flags(), &paths) {
        let wide = wide_patterns().iter().any(|p| path.contains(p));
        let want = if wide { wide_ms() } else { default_ms };
        assert_eq!(
            ms, want,
            "the runner gives {path} {ms} ms; it is {}named in WIDE_FAMILIES, so \
             it should get {want} ms",
            if wide { "" } else { "not " }
        );
    }
}

// The two dashboards loop a shape under a heap gauge until each interval
// converges, which is tens of seconds of region work on any tier and more under
// the eager profile. They used to run outside the batches under a budget of
// their own. Now they run in the implementation suite like every other file,
// so the wide budget is the only thing between them and the default.
//
// The counter-factual: fold the dashboards into the suite and name neither,
// and each is killed at the default on a loaded runner with no verdict
// recorded.
#[test]
fn the_dashboards_run_under_the_wide_budget() {
    let dashboards = [
        "tests/impl/oracle.lisp".to_string(),
        "tests/impl/plumb.lisp".to_string(),
    ];
    for (path, ms) in runner_budgets(&wide_flags(), &dashboards) {
        assert_eq!(
            ms,
            wide_ms(),
            "{path} is a dashboard and takes the wide budget, got {ms} ms"
        );
    }
}

/// Run `elle test` over a fixture that sleeps `SLEEP_S`, under `flags`, and say
/// whether the run gated green. The store is a scratch file: a fixture that
/// exists to time out must not land in the history every other run reads.
fn fixture_run(tag: &str, flags: &[&str]) -> (bool, String) {
    let dir = crate::common::ScratchDir::new(tag);
    // The `--wide` pattern is matched against the path, and the scratch path
    // ends in this name, so naming the file is what makes it wide.
    let fixture = dir.join("wide-budget-fixture.lisp");
    std::fs::write(&fixture, format!("(ev/sleep {SLEEP_S})\n")).expect("write the fixture");

    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("test")
        .arg(&fixture)
        .args(flags)
        .arg("--db")
        .arg(dir.join("s.db"))
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

/// Seconds the fixture sleeps: longer than `NARROW_MS`, shorter than `WIDE_MS`.
const SLEEP_S: u64 = 3;
const NARROW_MS: &str = "1000";
const WIDE_MS: &str = "15000";

// The budget has to reach the deadline, and only a form that runs can say that
// it did. Every other check here reads `--budget`, which answers from the flags
// alone — a runner that chose the budget correctly and then bound it nowhere
// would pass all of them and still kill every wide form at the default.
//
// The counter-factual is the second arm, and it is why the fixture sleeps
// rather than hangs: without it a pass would only say the sleep was short.
// Same fixture, same budgets, `--wide` withheld — and the form dies.
#[test]
fn a_wide_path_runs_a_form_the_default_budget_would_have_killed() {
    let budgets = ["--timeout", NARROW_MS, "--wide-timeout", WIDE_MS];

    let mut wide: Vec<&str> = budgets.to_vec();
    wide.extend(["--wide", "wide-budget-fixture.lisp"]);
    let (passed, output) = fixture_run("runner-budget-wide", &wide);
    assert!(
        passed,
        "a form of a --wide path must run to its end under --wide-timeout \
         ({WIDE_MS} ms), and this one was cut off at --timeout ({NARROW_MS} ms):\n{output}"
    );

    let (passed, output) = fixture_run("runner-budget-narrow", &budgets);
    assert!(
        !passed,
        "the same fixture, not named --wide, must be killed at {NARROW_MS} ms. \
         It passed, so the arm above proves nothing about the budget:\n{output}"
    );
    assert!(
        output.contains("timeout"),
        "the unnamed fixture must gate as a timeout, not some other failure:\n{output}"
    );
}

// A file that carries its own `deadline` reports through it — which request
// stalled, how long it waited — and that report is the reason to run the file.
// The runner's budget has to outlast it, or the runner's deadline lands first
// and the run records `timeout` with nothing else to say.
//
// The counter-factual is what this found: h2-stream-share.lisp gives itself
// 120 s and ran under the runner's 60 s default, so a slow box killed it
// mid-stream and CI reported a bare deadline against a file that was working.
#[test]
fn no_suite_file_outlives_the_runner_budget_before_its_own_deadline_fires() {
    let declaring: Vec<(String, u64)> = suite_files()
        .into_iter()
        .filter_map(|path| declared_deadline(&path).map(|d| (path, d)))
        .collect();
    assert!(
        !declaring.is_empty(),
        "no suite file declares a deadline. Either the declaration changed \
         shape or the argument no longer applies — teach this test the new \
         shape rather than letting it pass by matching nothing."
    );

    let paths: Vec<String> = declaring.iter().map(|(path, _)| path.clone()).collect();
    let budgets = runner_budgets(&wide_flags(), &paths);
    for (path, deadline) in declaring {
        let ms = budgets[&path];
        assert!(
            ms > deadline * 1000,
            "{path} gives itself {deadline} s to report a stall, and the runner \
             kills the form at {ms} ms. The runner's deadline lands first, so \
             the file's own diagnostic can never print — the run records \
             `timeout` and the reason names the budget, not the request that \
             stalled."
        );
    }
}
