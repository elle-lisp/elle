// audited: 2026-09-29
// What `.github/workflows/pr.yml` claims to gate must be what it gates, and
// what each job builds must let its own checks run.
//
// docs/analysis/ci.md
// .github/BRANCH_PROTECTION.md
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
// Tests job per platform, never both in one — is in docs/analysis/ci.md.

use crate::common::{runs_target, workflow_jobs as jobs};
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
    // well over five jobs.
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
// docs/analysis/ci.md.
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

// The `plugins/` submodule is a separate workspace, and until this job existed
// no CI job checked it out. A change to the `elle_api!` signatures stopped 17
// plugins from compiling, at about 90 call sites, and every gate stayed green.
// Plugins take `elle-plugin` by path, so a source break never reaches a load and
// the ABI version guard cannot see it; only a job that compiles the submodule
// can. The argument is in docs/analysis/ci.md.
//
// The counter-factual: this is the state main was in. Delete the job, or leave
// it building a `plugins/` it never checked out, and an ABI change breaks every
// plugin with nothing going red.
//
// The target matters as much as the job. `make plugins` builds `PORTABLE`,
// which is five plugins short of the workspace, so it reproduces the same
// silence for `elle-arrow`, `elle-polars`, `elle-vulkan`, `elle-egui` and
// `elle-wayland`. The trap: `make plugins-all` carries `make plugins` as a
// prefix, so a search for the shorter string accepts either target and lets
// that substitution through.
#[test]
fn a_job_builds_every_plugin_in_the_submodule() {
    let text = workflow_text();

    let building: Vec<(String, String)> = jobs(&text)
        .into_iter()
        .filter(|(_, body)| body.contains("make plugins-all") && body.contains("make smoke-plugins"))
        .collect();
    assert!(
        !building.is_empty(),
        "no job in {} runs both `make plugins-all` and `make smoke-plugins`, so \
         nothing in CI compiles the whole `plugins/` workspace against this \
         tree's `elle-plugin`",
        workflow_path().display()
    );

    // Without a checkout the submodule directory is empty, `make plugins`
    // builds nothing, and `plugins-verify` is the step that says so.
    for (name, body) in &building {
        assert!(
            body.contains("submodule"),
            "job `{name}` builds the plugins but never checks the submodule \
             out, so it runs over an empty directory"
        );
    }
}

// The region checks are `#[cfg(debug_assertions)]`, and every Makefile-driven
// job builds `--release`. So a corpus job without
// `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS` drives the whole corpus with those
// checks compiled out, and reports green over what they would have caught. The
// argument is in docs/analysis/ci.md.
//
// The trap: a green corpus job is not evidence the checks ran. `macOS Smoke`
// was the only job setting the flag, so both Linux backends ran blind and the
// slowest runner in the workflow was the sole detector — on a box whose
// failures read as flaky timeouts.
//
// The counter-factual: assert only that SOME job sets the flag, and the
// macOS-only state passes unchanged. The backend split below is the assertion.
#[test]
fn each_io_backend_has_a_linux_corpus_job_with_debug_assertions() {
    const FLAG: &str = "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS";
    let text = workflow_text();

    // A corpus job drives the Makefile's smoke targets. Restricted to Linux
    // runners: the point is that neither backend depends on the macOS box.
    let corpus: Vec<(String, String)> = jobs(&text)
        .into_iter()
        .filter(|(_, body)| body.contains("runs-on: ubuntu"))
        .filter(|(_, body)| body.contains("make smoke"))
        .collect();
    assert!(
        corpus.len() > 1,
        "found {} Linux corpus jobs in {}; the parse is broken, not the workflow",
        corpus.len(),
        workflow_path().display()
    );

    // `smoke-pool` builds without the `uring` feature, so it is the thread-pool
    // target; every other smoke target builds the backend a Linux build carries,
    // which is io_uring.
    let (pool, uring): (Vec<_>, Vec<_>) = corpus
        .iter()
        .partition(|(_, body)| runs_target(body, "smoke-pool"));

    for (backend, group) in [("thread-pool", &pool), ("io_uring", &uring)] {
        let covered: Vec<&String> = group
            .iter()
            .filter(|(_, body)| body.contains(FLAG))
            .map(|(name, _)| name)
            .collect();
        assert!(
            !covered.is_empty(),
            "no Linux corpus job sets `{FLAG}` on the {backend} backend, so the \
             region checks are compiled out of every {backend} run and only the \
             macOS job can catch what they find. Candidates: {:?}",
            group.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        );
    }
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

/// Each build CI makes, as the make target that builds it and runs the language
/// suite on it, and what the build is.
const BUILDS: &[(&str, &str)] = &[
    ("smoke-lang", "the default build"),
    ("smoke-nojit", "the build with no JIT"),
    ("smoke-pool", "the thread-pool I/O build"),
    ("smoke-mlir", "the MLIR build"),
    ("smoke-noffi", "the build with no features"),
];

// A build is one implementation, and the language suite is what every
// implementation must pass (docs/spec.md). The argument is in
// docs/analysis/ci.md.
//
// The counter-factual: drop the No-JIT job, and the interpreter alone is a
// runtime no gate runs. Every other job compiles hot functions, so a defect
// that only the interpreter shows reaches main green.
#[test]
fn every_build_runs_the_language_suite_in_a_job() {
    let text = workflow_text();
    let all = jobs(&text);
    for (target, build) in BUILDS {
        assert!(
            all.iter().any(|(_, body)| runs_target(body, target)),
            "no job in {} runs `make {target}`, so {build} never runs the \
             language suite",
            workflow_path().display()
        );
    }
}

// The implementation suite runs on the rig beside the default build, and again
// on the thread-pool build's rig, where the worker threads some of its files
// count exist. `make smoke` runs it too, on the platforms whose Smoke job runs
// that target.
#[test]
fn the_implementation_suite_runs_on_a_linux_job() {
    let text = workflow_text();
    let impl_jobs: Vec<String> = jobs(&text)
        .into_iter()
        .filter(|(_, body)| body.contains("runs-on: ubuntu"))
        .filter(|(_, body)| runs_target(body, "smoke-impl"))
        .map(|(name, _)| name)
        .collect();
    assert!(
        !impl_jobs.is_empty(),
        "no Linux job runs `make smoke-impl`, so the implementation suite runs \
         only on the platforms whose Smoke job runs `make smoke`"
    );
}

// The rig is a workspace crate with tests of its own: what it reads from a
// sidecar, what it refuses, and the recursions that need a mode. `cargo test
// --test '*'` runs the root package's tests alone, so a Rust Tests job that
// stops there never builds the rig's.
//
// The counter-factual: a sidecar key the rig starts ignoring runs every file
// that names it under the defaults, and the implementation suite reports a
// pass. Only the rig's own tests would see it.
#[test]
fn every_rust_test_job_runs_the_rigs_tests() {
    let text = workflow_text();
    let rust_jobs: Vec<(String, String)> = jobs(&text)
        .into_iter()
        .filter(|(_, body)| body.contains("cargo test --test '*'"))
        .collect();
    assert!(
        rust_jobs.len() > 1,
        "found {} Rust test jobs in {}; the parse is broken, not the workflow",
        rust_jobs.len(),
        workflow_path().display()
    );
    for (name, body) in rust_jobs {
        assert!(
            body.contains("cargo test -p elle-rig"),
            "job `{name}` runs the Rust suite and not `cargo test -p elle-rig`, \
             so the rig's tests run nowhere on its platform"
        );
    }
}

// The scrub profile zeroes each released page, and the panic that reads the
// zeros is `#[cfg(debug_assertions)]`, so the pass needs both
// (docs/impl/region/diagnostics.md). macOS is the runner where stale reads
// surface as wedges on the thread-pool backend, which is why the pass runs
// there.
//
// The counter-factual: the macOS job used to set `ELLE_TEST_FLAGS`, a variable
// the suite passes no longer read. A job that sets a variable nothing reads
// runs green and scrubs nothing.
#[test]
fn the_macos_smoke_job_runs_the_scrub_profile() {
    let text = workflow_text();
    let (name, body) = jobs(&text)
        .into_iter()
        .find(|(_, body)| body.contains("runs-on: macos") && runs_target(body, "smoke"))
        .expect("no macOS job runs `make smoke`");
    let profile = "tests/impl/profiles/scrub.toml";
    assert!(
        crate::common::repo_root().join(profile).exists(),
        "{profile} is gone"
    );
    let line = body
        .lines()
        .find(|line| runs_target(line, "smoke"))
        .unwrap_or_default();
    assert!(
        line.contains(&format!("IMPL_PROFILES={profile}")),
        "job `{name}` runs `make smoke` without the scrub profile:\n  {}",
        line.trim()
    );
    assert!(
        body.contains("CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS"),
        "job `{name}` scrubs pages in a build that cannot panic on reading one"
    );
}

// The MLIR job builds `elle-mlir` and never `elle` (bins/overview.md), so a
// `make doctest` there has to name the binary it runs. The counter-factual: a
// bare `make doctest` runs `target/release/elle`, which that job never built,
// and the doctests stop running on the MLIR build.
#[test]
fn the_mlir_job_runs_the_doctests_on_the_mlir_build() {
    let text = workflow_text();
    let (name, body) = jobs(&text)
        .into_iter()
        .find(|(_, body)| runs_target(body, "smoke-mlir"))
        .expect("no job runs `make smoke-mlir`");
    let line = body
        .lines()
        .find(|line| line.trim_start().starts_with("run:") && runs_target(line, "doctest"))
        .unwrap_or_else(|| panic!("job `{name}` runs no `make doctest`"));
    assert!(
        line.contains("ELLE=./target/release/elle-mlir"),
        "job `{name}` runs the doctests on a binary it never built:\n  {}",
        line.trim()
    );
}
