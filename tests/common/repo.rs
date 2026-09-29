// audited: 2026-09-29
//! What the Makefile and the two Elle suites say, read the way a pass reads them.
//!
//! tests/AGENTS.md
//! docs/testing.md
//!
//! Several test files ask the same questions of the two Elle suites and the
//! Makefile: which files a suite holds, which of them declare a deadline of
//! their own, what a variable expands to, and what a target will run. The
//! readers live here because a second copy of them is a second answer.

/// One Makefile variable's value, as `make` itself expands it, or `None` if
/// `make` could not be run.
///
/// Tests that check how CI dimensions the suites — job counts, per-file
/// budgets — have to read the value the pass will use, not the assignment's
/// text. Those two differ: a variable can sit inside an `ifdef`, be computed by
/// `$(shell …)`, be continued across lines with a backslash, or reference
/// another variable. A test that parses the Makefile reimplements `make`, and
/// the first thing such a parser does is disagree with it. This asks `make`.
///
/// `env` sets variables for the child, over a slate cleared of `GITHUB_ACTIONS`
/// and `JOBS`: the answer must not depend on whether the suite itself is
/// running under CI.
///
/// The Makefile's `print-%` rule is what this reads through.
#[allow(dead_code)]
pub fn make_var(name: &str, env: &[(&str, &str)]) -> Option<String> {
    let mut command = std::process::Command::new("make");
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--no-print-directory")
        .arg(format!("print-{name}"))
        .env_remove("GITHUB_ACTIONS")
        .env_remove("JOBS");
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

/// The commands `make TARGET` will run, expanded, without running any of them.
///
/// `make_var` above answers one variable; a recipe is where those variables
/// meet the flags written beside them, and a test about what a pass actually
/// runs has to read the whole line. `--dry-run` is what prints it: every
/// variable resolved, every `@` line shown, nothing executed.
///
/// The child runs over a slate cleared of `GITHUB_ACTIONS` and `JOBS`, for the
/// reason `make_var` clears them.
#[allow(dead_code)]
pub fn make_dry_run(target: &str) -> Option<String> {
    make_dry_run_with(target, &[])
}

/// The commands `make TARGET VARS…` will run: [`make_dry_run`] with variables
/// set on the command line, the way a CI job overrides one.
#[allow(dead_code)]
pub fn make_dry_run_with(target: &str, vars: &[&str]) -> Option<String> {
    let out = std::process::Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--dry-run")
        .arg("--no-print-directory")
        .arg(target)
        .args(vars)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("JOBS")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// The repository root, as the test binary was compiled against it.
#[allow(dead_code)]
pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The Makefile's text.
#[allow(dead_code)]
pub fn makefile() -> String {
    std::fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile")
}

/// One Makefile variable, as `make` expands it.
///
/// Asking `make` rather than parsing the assignment is the whole point: these
/// tests measure what a pass will actually run, and a parser that reimplements
/// variable references, line continuations and `$(shell …)` is a second `make`
/// that can disagree with the first.
#[allow(dead_code)]
pub fn make_expand(name: &str) -> String {
    make_var(name, &[]).unwrap_or_else(|| panic!("`make print-{name}` did not run"))
}

/// The path patterns the Makefile gives the wider budget.
///
/// A pattern is a substring of a path, not a file name: a whole family of
/// files shares one deadline and one prefix, so the list names the prefix
/// rather than every member.
#[allow(dead_code)]
pub fn wide_patterns() -> Vec<String> {
    let names: Vec<String> = make_expand("WIDE_FAMILIES")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    assert!(!names.is_empty(), "WIDE_FAMILIES names no family");
    names
}

/// Every file the suite in `dir` runs, as a repo-relative path. A suite runs
/// the files at the top of its directory, and nothing below it.
#[allow(dead_code)]
pub fn suite(dir: &str) -> Vec<String> {
    let mut paths: Vec<String> = std::fs::read_dir(repo_root().join(dir))
        .unwrap_or_else(|e| panic!("read {dir}: {e}"))
        .map(|entry| entry.expect("a suite directory entry").file_name())
        .filter_map(|name| name.to_str().map(str::to_string))
        .filter(|name| name.ends_with(".lisp"))
        .map(|name| format!("{dir}/{name}"))
        .collect();
    paths.sort();
    assert!(
        paths.len() > 100,
        "the suite in {dir} did not read: {paths:?}"
    );
    paths
}

/// Every file of both Elle suites, as repo-relative paths.
#[allow(dead_code)]
pub fn suite_files() -> Vec<String> {
    let mut paths = suite("tests/lang");
    paths.extend(suite("tests/impl"));
    paths
}

/// The deadline a suite file gives itself, if it declares one.
///
/// A file that has to detect a stall carries `(def deadline N)` and reports
/// through it — which request stalled, and how long it waited. That number is
/// in seconds, and it is the only thing that knows what the file considers
/// hung.
#[allow(dead_code)]
pub fn declared_deadline(path: &str) -> Option<u64> {
    let source = std::fs::read_to_string(repo_root().join(path)).expect("read a suite file");
    let (_, rest) = source.split_once("(def deadline ")?;
    let digits = rest.split(')').next()?.trim();
    Some(
        digits
            .parse()
            .unwrap_or_else(|_| panic!("{path} declares a deadline this cannot read: {digits}")),
    )
}
