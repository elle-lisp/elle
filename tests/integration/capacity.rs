// audited: 2026-09-29
// The corpus sizes a CI runner is given: processes at once, and files per `elle test` process.
//
// A wrong job count fails no assertion, so nothing but this file catches one.
//
// The job-count expectation is independent of the Makefile's own arithmetic:
// the Makefile asks the shell, this asks Rust. Two different counts mean the
// Makefile is reading something other than the machine it runs on.
//
// The trap in that independence: the two agree on a GitHub runner and on a bare
// box, and can disagree under a cgroup CPU quota. `available_parallelism`
// honours a quota; `nproc` and `getconf` report the processors that exist. A
// container with `--cpus=2` on a 32-way host fails the first test below — which
// is the right answer, because the Makefile really would ask that container for
// 32 corpus processes.
//
// docs/analysis/ci.md

use crate::common::make_var;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The environment a CI runner presents to the Makefile.
const CI: &[(&str, &str)] = &[("GITHUB_ACTIONS", "true")];

/// The processors this box will actually schedule on, asked of the platform
/// rather than of the Makefile.
fn processors() -> usize {
    std::thread::available_parallelism()
        .expect("the platform reports its parallelism")
        .get()
}

// The CI job count tracks the runner. The counter-factual is the defect this
// replaced: `JOBS ?= 4`, which matched the four-processor Linux runners exactly
// and asked the three-processor macOS runner for a third more work than it had.
// A constant passes every gate on the box it was chosen for.
#[test]
fn the_ci_job_count_is_read_from_the_runner() {
    let Some(jobs) = make_var("JOBS", CI) else {
        eprintln!("SKIPPED: `make print-JOBS` did not run");
        return;
    };
    assert_eq!(
        jobs,
        processors().to_string(),
        "under GITHUB_ACTIONS the Makefile runs {jobs} corpus processes at \
         once, and this box has {} processors. A job count that does not \
         track the runner over-subscribes whichever runner it was not chosen \
         for; see docs/analysis/ci.md § \"Runner capacity\".",
        processors()
    );
}

// An override has to survive the detection, or a job that needs a different
// number has no way to ask for one — and the wasm pass, which is bounded by
// memory rather than by processors, is exactly such a job.
#[test]
fn an_explicit_job_count_overrides_the_detection() {
    let Some(jobs) = make_var("JOBS", &[("GITHUB_ACTIONS", "true"), ("JOBS", "3")]) else {
        eprintln!("SKIPPED: `make print-JOBS` did not run");
        return;
    };
    assert_eq!(
        jobs, "3",
        "JOBS=3 in the environment did not reach the corpus passes. The \
         assignment must stay `?=`, or a runner that needs a smaller number \
         cannot ask for one."
    );
}

// Outside CI the default is deliberately a constant, not the box's processor
// count: a development box is not sized by the runner and shares its cores with
// everything else its owner is running. This pins the distinction, so a change
// that makes CI adaptive does not silently take the local default with it.
#[test]
fn the_local_default_stays_a_constant() {
    let Some(jobs) = make_var("JOBS", &[]) else {
        eprintln!("SKIPPED: `make print-JOBS` did not run");
        return;
    };
    assert!(
        jobs.parse::<usize>().is_ok_and(|n| n > 0),
        "the local JOBS default is not a positive number: {jobs}"
    );
    assert_eq!(
        jobs, "16",
        "the local default job count changed to {jobs}. That is a choice about \
         development boxes, not about CI runners — make it deliberately and \
         update docs/analysis/ci.md § \"Runner capacity\", which records 16."
    );
}

// The corpus batch size, per platform. `HOST_OS` is the seam: it defaults to
// `uname -s`, and these tests set it to present a platform the suite is not
// running on. Without the seam only the platform running the suite is
// checked, and the macOS choice would go unchecked on every Linux box.
//
// The counter-factual is a single constant: 25 fits every runner except the one
// that stalled, and passes every gate on the box it was chosen for.
fn batch(env: &[(&str, &str)]) -> Option<String> {
    let value = make_var("CORPUS_BATCH", env);
    if value.is_none() {
        eprintln!("SKIPPED: `make print-CORPUS_BATCH` did not run");
    }
    value
}

#[test]
fn the_macos_corpus_batch_is_ten_files() {
    let Some(files) = batch(&[("HOST_OS", "Darwin")]) else {
        return;
    };
    assert_eq!(
        files, "10",
        "on Darwin the Makefile deals {files} files to each `elle test` \
         process, and the macOS choice is 10; see docs/analysis/ci.md \
         § \"Corpus batch size\"."
    );
}

#[test]
fn the_linux_corpus_batch_stays_twenty_five_files() {
    let Some(files) = batch(&[("HOST_OS", "Linux")]) else {
        return;
    };
    assert_eq!(
        files, "25",
        "on Linux the Makefile deals {files} files to each `elle test` \
         process. The smaller batch is a macOS mitigation, and it costs a \
         process start per batch everywhere it applies."
    );
}

#[test]
fn an_explicit_corpus_batch_overrides_the_platform() {
    let Some(files) = batch(&[("HOST_OS", "Darwin"), ("CORPUS_BATCH", "7")]) else {
        return;
    };
    assert_eq!(
        files, "7",
        "CORPUS_BATCH=7 in the environment did not reach the corpus passes. \
         The assignment must stay `?=`, or a job that needs a different \
         batch cannot ask for one."
    );
}

// The rule the other tests read through. A `print-%` that stops existing takes
// every assertion above it with it: `make` fails, each test prints SKIPPED, and
// the job count goes unchecked on every platform at once.
#[test]
fn the_makefile_keeps_the_rule_these_tests_read_variables_through() {
    let text = fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile");
    assert!(
        text.contains("print-%:"),
        "the Makefile no longer defines `print-%`. Every test in this file \
         reads a variable through it and skips without it, so the job count \
         would stop being checked with nothing reporting that."
    );
}
