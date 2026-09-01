// What `.github/workflows/pr.yml` claims to gate must be what it gates.
//
// The workflow is not compiled and not linted. GitHub checks that a `needs`
// entry names a real job and stops there, so both halves of the merge gate can
// rot without anything reporting it: a job absent from `all-checks`'s `needs`
// runs and reports next to a pull request it can never block, and a job named
// in `needs` with no `exit 1` step is waited for and then ignored. Branch
// protection requires the single "All Checks Passed" context
// (.github/BRANCH_PROTECTION.md), so that job's hand-maintained list IS the
// gate. These tests are the standing check on it.
//
// The argument for the shape the second test pins — one Smoke job and one Rust
// Tests job per platform, never both in one — is in docs/analysis/ci.md
// § "Why each platform has two test jobs".

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/pr.yml")
}

fn workflow_text() -> String {
    fs::read_to_string(workflow_path())
        .unwrap_or_else(|e| panic!("read {}: {e}", workflow_path().display()))
}

/// `  name:` at exactly two spaces of indent — a key in the `jobs` mapping.
/// A comment at that indent has no bare identifier before the colon, so the
/// character check rejects it.
fn job_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let name = rest.strip_suffix(':')?;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if name.is_empty() || !name.chars().all(ident) {
        return None;
    }
    Some(name.to_string())
}

/// Every job in the workflow, as (name, body). Comment lines are dropped from
/// the body: the trap is that several jobs discuss commands they do not run —
/// the WASM job's comment names `make smoke-wasm` while running `check-wasm` —
/// so a body scan that kept comments would read those as steps.
fn jobs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut in_jobs = false;

    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        // Any other top-level key closes the `jobs` mapping.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        if let Some(name) = job_header(line) {
            out.extend(current.take());
            current = Some((name, String::new()));
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            if !line.trim_start().starts_with('#') {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    out.extend(current);
    out
}

/// The gate job: the one whose result branch protection requires.
const GATE: &str = "all-checks";

fn gate_body(text: &str) -> String {
    jobs(text)
        .into_iter()
        .find(|(name, _)| name == GATE)
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("{} defines no `{GATE}` job", workflow_path().display()))
}

/// The jobs `all-checks` waits for, from its `needs: [...]` list.
fn declared_needs(gate: &str) -> BTreeSet<String> {
    let (_, rest) = gate
        .split_once("needs: [")
        .expect("`all-checks` declares `needs` as a bracketed list");
    let (inner, _) = rest.split_once(']').expect("the `needs` list closes");
    inner
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The jobs `all-checks` actually fails on, from its `needs.NAME.result` steps.
fn enforced_results(gate: &str) -> BTreeSet<String> {
    gate.split("needs.")
        .skip(1)
        .filter_map(|rest| rest.split_once(".result").map(|(name, _)| name.to_string()))
        .collect()
}

// A job outside `all-checks`'s `needs` burns runner minutes and reports a status
// nothing reads: the required context is "All Checks Passed", which never waited
// for it. The counter-factual: add a job that runs `exit 1`, leave the `needs`
// list alone, and every gate ahead of the merge still goes green.
#[test]
fn all_checks_waits_for_every_job() {
    let text = workflow_text();
    let defined: BTreeSet<String> = jobs(&text)
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != GATE)
        .collect();

    // A parse that matched nothing would assert nothing. The workflow has run
    // well over five jobs since the platform tiers landed.
    assert!(
        defined.len() > 5,
        "found only {} jobs in {}; the parse is broken, not the workflow",
        defined.len(),
        workflow_path().display()
    );

    let needs = declared_needs(&gate_body(&text));
    let ungated: Vec<_> = defined.difference(&needs).collect();
    assert!(
        ungated.is_empty(),
        "these jobs are outside `{GATE}`'s `needs` list, so they run on every \
         pull request and cannot block one: {ungated:?}"
    );

    let stale: Vec<_> = needs.difference(&defined).collect();
    assert!(
        stale.is_empty(),
        "`{GATE}` needs these, which no job defines: {stale:?}"
    );
}

// The other half: a `needs` entry makes `all-checks` WAIT for a job, and
// nothing more. `if: always()` means the gate then reports success unless some
// step fails it, so a job with no `needs.NAME.result` step is a job whose
// failure is waited for and discarded. The counter-factual: delete the "Check
// macOS" step, keep `macos` in `needs`, and a red macOS job merges green.
#[test]
fn all_checks_fails_on_every_job_it_waits_for() {
    let gate = gate_body(&workflow_text());
    let needs = declared_needs(&gate);
    let enforced = enforced_results(&gate);

    let unenforced: Vec<_> = needs.difference(&enforced).collect();
    assert!(
        unenforced.is_empty(),
        "`{GATE}` waits for these jobs but has no step failing on their result, \
         so their failures are discarded: {unenforced:?}"
    );

    let orphaned: Vec<_> = enforced.difference(&needs).collect();
    assert!(
        orphaned.is_empty(),
        "`{GATE}` reads the result of these jobs without needing them, so it may \
         read them before they finish: {orphaned:?}"
    );
}

// The corpus and the Rust suite share no build and no run, so a job that does
// both costs the sum. The pull request waits for the slowest job, which makes
// any such job the wall clock for its whole platform. Splitting the pair into
// two jobs trades runner minutes for that wall clock — the argument is in
// docs/analysis/ci.md § "Why each platform has two test jobs".
//
// The counter-factual: before the split, `macos` and `aarch64` each ran
// `make smoke` and then `cargo test`, and every other gate stayed green while
// the slowest platform took twice the time it needed.
#[test]
fn no_job_serializes_the_corpus_and_the_rust_suite() {
    let text = workflow_text();
    let both: Vec<String> = jobs(&text)
        .into_iter()
        .filter(|(_, body)| body.contains("make smoke") && body.contains("cargo test"))
        .map(|(name, _)| name)
        .collect();

    assert!(
        both.is_empty(),
        "these jobs run the corpus and the Rust suite in series, so their \
         platform costs the sum of the two instead of the slower one: {both:?}. \
         Split each into a Smoke job and a Rust Tests job."
    );
}

// A split pair that shares one `Swatinem/rust-cache` `shared-key` is worse than
// no cache: the Smoke job populates the key with release artifacts, the Rust
// Tests job restores those and saves dev-profile ones over them, and the two
// overwrite each other on alternating runs. Every job that names a `shared-key`
// must name its own.
#[test]
fn no_two_jobs_share_a_cache_key() {
    let text = workflow_text();
    let mut seen: Vec<(String, String)> = Vec::new();

    for (name, body) in jobs(&text) {
        for part in body.split("shared-key: ").skip(1) {
            let key = part.lines().next().unwrap_or("").trim().to_string();
            if let Some((other, _)) = seen.iter().find(|(_, k)| *k == key) {
                panic!(
                    "jobs `{other}` and `{name}` both cache under shared-key \
                     `{key}`, so each run saves over the other's artifacts"
                );
            }
            seen.push((name.clone(), key));
        }
    }

    assert!(
        !seen.is_empty(),
        "no job names a shared-key; the parse is broken, not the workflow"
    );
}
