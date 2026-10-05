// audited: 2026-10-05
// The default build's suite targets: which files each pass runs, on which
// program, under which flags.
//
// docs/testing.md
// rig/overview.md
//
// Every assertion reads `make --dry-run`, which prints each command a target
// will run with every variable resolved. A target that names the wrong files or
// passes a flag the language suite must never see still runs, still records a
// verdict per file, and still gates green, so the recipe is checked here.

use crate::common::{
    assert_plain_language_pass, assert_producer_pass, assert_rig_runs, isolated_impl_files,
    lang_files, make_expand, makefile, passes, Pass,
};
use std::collections::BTreeSet;

const EAGER: &str = "tests/impl/profiles/jit-eager.toml";
const SCRUB: &str = "tests/impl/profiles/scrub.toml";
const ACCEPTANCE: [&str; 2] = ["tests/runner/tiers.lisp", "tests/runner/acceptance.lisp"];

// The language suite runs the user build as shipped, inside the runner, which
// runs each file on every tier the build carries (docs/test-runner.md).
//
// The counter-factual: a pass that isolates each file as `elle FILE` runs it
// once, on whatever the runtime picks, and no disagreement between tiers is
// ever recorded.
#[test]
fn smoke_lang_runs_every_language_file_with_no_flag() {
    let passes = passes("smoke-lang", &[]);
    assert_eq!(passes.len(), 1, "`make smoke-lang` is one pass");
    assert_plain_language_pass("smoke-lang", &passes[0], "ELLE");
}

// The implementation suite runs on the rig, so each file's sidecar sets its
// mode, and the runner is the rig itself, so the run has the rig's build and
// its readings are judged (docs/ratchet.md). The runner's acceptance tests
// ride the same pass: they drive `elle test` themselves, and they need the
// store the pass records into.
//
// The counter-factual: run the implementation suite under `elle`, and every
// sidecar goes unread. The guardfree files run with the oracle disarmed and
// pass. Run it under `elle test --host elle-rig`, and the run has no build, so
// every reading goes unrecorded and every ledger row unjudged.
#[test]
fn smoke_impl_runs_the_implementation_suite_on_the_rig() {
    let passes = passes("smoke-impl", &[]);
    let base = &passes[0];
    let mut want = isolated_impl_files();
    want.extend(ACCEPTANCE.map(str::to_string));
    assert_eq!(
        base.files, want,
        "the first pass of `make smoke-impl` is the implementation suite and \
         the runner's acceptance tests"
    );
    assert_eq!(
        base.isolate(),
        Some(""),
        "each sidecar alone sets its file's mode"
    );
    assert_rig_runs(base, "ELLE_RIG", "the implementation suite");
}

// The eager profile runs both suites with every function compiled on its first
// call. A profile's `jit` replaces a sidecar's, so the pinned files meet the
// eager tier here too.
#[test]
fn smoke_impl_runs_both_suites_under_the_eager_profile() {
    let passes = passes("smoke-impl", &[]);
    let eager: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.isolate() == Some(format!("--profile {EAGER}").as_str()))
        .collect();
    assert_eq!(eager.len(), 1, "one pass runs the eager profile");
    let want: BTreeSet<String> = lang_files()
        .union(&isolated_impl_files())
        .cloned()
        .collect();
    assert_eq!(eager[0].files, want, "the eager profile runs both suites");
    assert_rig_runs(eager[0], "ELLE_RIG", "the eager profile, a rig setting,");
}

// The producers run in-process on the rig, in a pass of their own, so each
// reading comes from both JIT policies (docs/test-runner.md).
//
// The counter-factual: an isolated producer runs once, under whatever its
// child's policy is, so a reading that moves under the eager JIT is read only
// in a second pass, and a producer that runs in both kinds of pass is read
// twice under one policy.
#[test]
fn smoke_impl_runs_the_producers_in_process_in_a_pass_of_their_own() {
    assert_producer_pass("smoke-impl", &passes("smoke-impl", &[]), "ELLE_RIG");
}

// `IMPL_PROFILES` names further profiles for the language suite. The macOS job
// sets it to the scrub profile (tests/integration/workflows.rs).
#[test]
fn a_named_profile_runs_the_language_suite_on_the_rig() {
    let var = format!("IMPL_PROFILES={SCRUB}");
    let passes = passes("smoke-impl", &[&var]);
    let scrub: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.isolate() == Some(format!("--profile {SCRUB}").as_str()))
        .collect();
    assert_eq!(scrub.len(), 1, "one pass runs the named profile");
    assert_eq!(
        scrub[0].files,
        lang_files(),
        "a named profile runs the language suite"
    );
    assert_rig_runs(scrub[0], "ELLE_RIG", "a named profile");
}

// A profile a pass names and nothing holds reads as no file at all: the rig
// refuses it, and the pass fails every file for a reason that is not the test.
#[test]
fn every_profile_a_pass_names_exists() {
    let named = passes("smoke-impl", &[&format!("IMPL_PROFILES={SCRUB}")]);
    for pass in named.into_iter().chain(passes("smoke-wasm", &[])) {
        if let Some(path) = pass
            .isolate()
            .and_then(|flags| flags.strip_prefix("--profile "))
        {
            assert!(
                crate::common::repo_root().join(path).exists(),
                "a pass names the profile {path}, which does not exist"
            );
        }
    }
}

// The boot image's gate runs the language suite on an instance that hydrates
// its image instead of compiling the stdlib, and changes nothing else.
#[test]
fn smoke_boot_image_runs_the_language_suite_from_the_image() {
    let passes = passes("smoke-boot-image", &[]);
    assert_eq!(passes.len(), 1);
    assert_eq!(passes[0].files, lang_files());
    assert_eq!(
        passes[0].isolate(),
        Some(format!("--boot-image={}", make_expand("BOOT_IMAGE_DIR")).as_str()),
        "each file boots from the stored image"
    );
}

// `make smoke` is what the merge queue runs, and what a contributor runs
// before a push. It carries both suites.
#[test]
fn smoke_runs_both_suites() {
    let text = makefile();
    let line = text
        .lines()
        .find(|l| l.starts_with("smoke:"))
        .expect("the Makefile defines `smoke`");
    let deps: Vec<&str> = line
        .split_once(':')
        .map(|(_, rest)| rest.split('#').next().unwrap_or(""))
        .unwrap_or("")
        .split_whitespace()
        .collect();
    for want in [
        "smoke-lang",
        "smoke-impl",
        "doctest",
        "embedding",
        "semver-check",
    ] {
        assert!(
            deps.contains(&want),
            "`make smoke` does not run {want}: {line}"
        );
    }
}
