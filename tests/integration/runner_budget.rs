// audited: 2026-09-21
// The wall-clock budget `elle test` gives one form, and how the corpus pass
// tells it which paths need a wider one.
//
// The per-file passes beside this one pick a budget in the shell, per file, and
// tests/integration/budget.rs checks that half. `RUN_CORPUS` cannot: one runner
// process takes a whole batch of files, so no shell sees a path in time to
// choose. The policy travels to the runner as flags instead, and this file
// checks that it arrives and that the runner acts on it.
//
// Three places have to agree, and nothing compiles any of them. The Makefile
// names the families. The recipe passes them. The runner reads them when it
// sets a form's join deadline. Every pair of those can be right while the third
// is wrong, and the failure is the same either way: a file is killed at a
// budget narrower than its own deadline, so the diagnostic it exists to print
// never prints and CI reports a bare `timeout`.
//
// docs/test-cli.md

use crate::common::{
    budget_seconds, corpus_files, declared_deadline, make_dry_run, make_expand as expand,
    repo_root, wide_patterns,
};
use std::collections::BTreeMap;
use std::process::Command;

/// The flags `RUN_CORPUS` hands `elle test`, ready for a `Command`.
fn wide_flags() -> Vec<String> {
    let flags = expand("WIDE_FLAGS");
    let words: Vec<String> = flags.split_whitespace().map(str::to_string).collect();
    assert!(
        !words.is_empty(),
        "WIDE_FLAGS expands to nothing, so the corpus pass tells the runner no policy"
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
/// session store. Asking the binary is the argument budget.rs already makes for
/// `sh`: the selector belongs to the thing under test, and a copy of its rule
/// written here is a second runner that can disagree with the first.
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
    let path = "tests/elle/arithmetic.lisp".to_string();
    runner_budgets(&[], std::slice::from_ref(&path))[&path]
}

/// The command line `make smoke-elle` will run a corpus batch under.
///
/// `--dry-run` rather than the Makefile's text: a flag and its value meet on a
/// recipe line through several variables, and what the pass runs is the only
/// thing worth asserting on. A check that greps the source for `$(WIDE_FLAGS)`
/// passes on a variable that expands to nothing.
fn corpus_batch_line() -> String {
    let recipe = make_dry_run("smoke-elle").expect("`make --dry-run smoke-elle` did not run");
    recipe
        .lines()
        .find(|line| line.contains(" test ") && line.contains("xargs"))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("`make smoke-elle` runs no `elle test` batch:\n{recipe}"))
}

// A policy the recipe never passes is a policy the runner never hears. Every
// other check here drives the runner directly, so every other check can pass
// while the corpus still runs each form under the default.
//
// The counter-factual: teach the runner `--wide`, cover it, and leave
// `RUN_CORPUS` spelling `elle test` the way it always did.
//
// The batch line must also carry no bare `--timeout`. One wide `--timeout` for
// the whole batch clears every declared deadline and reads as a fix — and it
// hands 150 s to every ordinary file too, so a hang the gate used to catch in
// 60 s now costs two and a half minutes per form.
#[test]
fn the_corpus_pass_hands_the_runner_the_wide_policy() {
    let batch = corpus_batch_line();
    let flags = wide_flags().join(" ");
    assert!(
        batch.contains(&flags),
        "the corpus pass runs the runner without the wide-family policy:\n  {}\n\
         Every form in the batch then takes the runner's default budget, \
         including the families whose own deadline is wider than it.\n\
         Expected the line to carry: {flags}",
        batch.trim()
    );
    assert!(
        !batch.contains("--timeout"),
        "the corpus pass widens every form in the batch with one `--timeout`:\n  {}\n\
         The named families need the wider budget; every other file needs the \
         default, which is what makes a hang fail fast.",
        batch.trim()
    );
}

// One list of families, two readers. `WIDE_FILES` is what the per-file passes
// grep with and `WIDE_FLAGS` is what the runner is told, so a family named to
// one and not the other widens on one pass and not the other.
//
// The counter-factual: add an `h2-window-` family to `WIDE_FILES` alone. The
// per-file passes give it the wide budget, the runner gives it the default, and
// the file dies under the runner with no account of which request stalled.
#[test]
fn both_budgets_read_one_list_of_families() {
    let flags = wide_flags();
    let named: Vec<String> = flags
        .windows(2)
        .filter(|pair| pair[0] == "--wide")
        .map(|pair| pair[1].clone())
        .collect();

    for pattern in wide_patterns() {
        assert!(
            named.contains(&pattern),
            "`{pattern}` is named in WIDE_FILES but not handed to the runner: {flags:?}"
        );
    }
    assert_eq!(
        named.len(),
        wide_patterns().len(),
        "the runner is told a different list of families than the per-file \
         passes grep with: {named:?} against {:?}",
        wide_patterns()
    );

    // The same budget in two spellings: `timeout` takes seconds and the runner
    // takes milliseconds. They are one number, or the two passes disagree about
    // how wide "wide" is.
    let wide = expand("WIDE_TIMEOUT");
    assert_eq!(
        wide_ms(),
        budget_seconds(&wide) * 1000,
        "WIDE_TIMEOUT is {wide} and WIDE_TIMEOUT_MS is {}, which is not the \
         same budget",
        wide_ms()
    );
}

// The runner's half, driven through the runner. A flag it accepts and ignores
// passes every check above — the Makefile names the families, the recipe passes
// them, and every form still runs under the default. Only the runner can say
// what a path's budget is.
//
// The counter-factual: parse the flags and never consult them where the join
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
    // the corpus never runs.
    let controls = ["tests/elle/arithmetic.lisp", "tests/elle/strings.lisp"];
    for path in controls {
        assert!(repo_root().join(path).exists(), "{path} is gone");
    }

    let mut paths: Vec<String> = controls.iter().map(|p| (*p).to_string()).collect();
    for pattern in wide_patterns() {
        paths.extend(
            corpus_files()
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
            "the runner gives {path} {ms} ms; it is {}named in WIDE_FILES, so \
             it should get {want} ms",
            if wide { "" } else { "not " }
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

// The budget has to reach the join deadline, and only a form that runs can say
// that it did. Every other check here reads `--budget`, which answers from the
// flags alone — a runner that chose the budget correctly and then bound it
// nowhere would pass all of them and still kill every wide form at the default.
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

// The claim budget.rs makes about the per-file passes, made about the runner. A
// file that carries its own `deadline` reports through it — which request
// stalled, how long it waited — and that report is the reason to run the file.
// The runner's budget has to outlast it, or the join deadline lands first and
// the run records `timeout` with nothing else to say.
//
// The counter-factual is what this found: h2-stream-share.lisp gives itself
// 120 s and ran under the runner's 60 s default, so a slow box killed it
// mid-stream and CI reported a bare deadline against a file that was working.
#[test]
fn no_corpus_file_outlives_the_runner_budget_before_its_own_deadline_fires() {
    let declaring: Vec<(String, u64)> = corpus_files()
        .into_iter()
        .filter_map(|path| declared_deadline(&path).map(|d| (path, d)))
        .collect();
    assert!(
        !declaring.is_empty(),
        "no corpus file declares a deadline. Either the declaration changed \
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
             kills the form at {ms} ms. The join deadline lands first, so the \
             file's own diagnostic can never print — the run records `timeout` \
             and the reason names the budget, not the request that stalled."
        );
    }
}
