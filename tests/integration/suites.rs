// audited: 2026-10-21
// The default build's suite targets: which files each pass runs, on which
// program, under which flags, and that each target is one pass.
//
// docs/testing.md
// docs/analysis/ci.md
// rig/overview.md
//
// Every assertion reads `make --dry-run`, which prints each command a target
// will run with every variable resolved. A target that names the wrong files or
// passes a flag the language suite must never see still runs, still records a
// verdict per file, and still gates green, so the recipe is checked here.

use crate::common::{
    assert_plain_language_pass, assert_producer_pass, assert_rig_runs, build_producer_files,
    charge_files, isolated_impl_files, lang_files, make_expand, makefile_rules, passes,
    suite_targets, the_pass, Pass, CHARGE_SKIP,
};
use std::collections::BTreeSet;

const EAGER: &str = "tests/impl/profiles/jit-eager.toml";
const SCRUB: &str = "tests/impl/profiles/scrub.toml";
const ACCEPTANCE: [&str; 2] = ["tests/runner/tiers.lisp", "tests/runner/acceptance.lisp"];

/// The default build's suite targets other than the language suite, each one
/// pass on its rig.
const DEFAULT_RIG_TARGETS: [&str; 5] = [
    "smoke-impl",
    "smoke-eager",
    "smoke-producers",
    "smoke-charge",
    "smoke-scrub",
];

// A target of several passes made a CI step of several passes: the step's time
// was their sum, and its log mixed their batch summaries with one `===` line
// between two passes (docs/analysis/ci.md).
//
// The counter-factual: `smoke-impl` ran four passes, `smoke-pool` and
// `smoke-mlir` three each, and every check of what each pass runs stayed green.
#[test]
fn every_suite_target_runs_exactly_one_pass() {
    let rules = makefile_rules();
    for target in &suite_targets() {
        let (_, recipe) = &rules[target];
        assert_eq!(
            recipe.matches("$(call RUN_SUITE").count(),
            1,
            "`make {target}`'s recipe calls RUN_SUITE more than once:\n{recipe}"
        );
        the_pass(target);
    }
}

// The language suite runs the user build as shipped, inside the runner, which
// runs each file on every tier the build carries (docs/test-runner.md).
//
// The counter-factual: a pass that isolates each file as `elle FILE` runs it
// once, on whatever the runtime picks, and no disagreement between tiers is
// ever recorded.
#[test]
fn smoke_lang_runs_every_language_file_with_no_flag() {
    assert_plain_language_pass("smoke-lang", &the_pass("smoke-lang"), "ELLE");
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
    let pass = the_pass("smoke-impl");
    let mut want = isolated_impl_files();
    want.extend(ACCEPTANCE.map(str::to_string));
    assert_eq!(
        pass.files, want,
        "`make smoke-impl` runs the implementation suite and the runner's \
         acceptance tests"
    );
    assert_eq!(
        pass.isolate(),
        Some(""),
        "each sidecar alone sets its file's mode"
    );
    assert_rig_runs(&pass, "ELLE_RIG", "the implementation suite");
}

// The eager profile runs both suites with every function compiled on its first
// call. A profile's `jit` replaces a sidecar's, so the pinned files meet the
// eager tier here too.
#[test]
fn smoke_eager_runs_both_suites_under_the_eager_profile() {
    let pass = the_pass("smoke-eager");
    assert_eq!(pass.isolate(), Some(format!("--profile {EAGER}").as_str()));
    let want: BTreeSet<String> = lang_files()
        .union(&isolated_impl_files())
        .cloned()
        .collect();
    assert_eq!(pass.files, want, "the eager profile runs both suites");
    assert_rig_runs(&pass, "ELLE_RIG", "the eager profile, a rig setting,");
}

// The producers run in-process on the rig, in a pass of their own, so each
// reading comes from both JIT policies (docs/test-runner.md). The two that read
// the tree run in `make qa` instead (docs/analysis/ci.md).
//
// The counter-factual: an isolated producer runs once, under whatever its
// child's policy is, so a reading that moves under the eager JIT is read only
// in a second pass, and a producer that runs in both kinds of pass is read
// twice under one policy.
#[test]
fn smoke_producers_runs_the_producers_in_process_in_a_pass_of_their_own() {
    let all: Vec<Pass> = DEFAULT_RIG_TARGETS.iter().map(|t| the_pass(t)).collect();
    assert_producer_pass("the default build's rig targets", &all, "ELLE_RIG");
    assert_eq!(
        the_pass("smoke-producers").files,
        build_producer_files(),
        "`make smoke-producers` runs every producer but the tree's"
    );
}

// Each file's charge on the runner's heap is read in a pass of its own, in
// process on the rig, over the language suite and the implementation files
// with no sidecar (docs/test-gauges.md).
//
// The counter-factual: an isolated pass charges the runner a spawn, so its
// charge says nothing about the file; and a file the pass drops keeps its
// rows in the ledger with nobody reading them and no `missing` to say so.
#[test]
fn smoke_charge_reads_each_files_charge_in_a_pass_of_its_own() {
    let pass = the_pass("smoke-charge");
    assert!(pass.charge(), "`make smoke-charge` reads the charge");
    assert_eq!(
        pass.files,
        charge_files(),
        "the charge pass runs the language suite and the sidecar-free \
         implementation files, less the producers and {CHARGE_SKIP}"
    );
    assert_eq!(pass.isolate(), None, "and runs them in-process");
    assert_rig_runs(&pass, "ELLE_RIG", "the charge pass");
}

// The scrub profile runs the language suite on the rig with each released page
// zeroed. The macOS job runs it (tests/integration/workflows.rs).
#[test]
fn smoke_scrub_runs_the_language_suite_under_the_scrub_profile() {
    let pass = the_pass("smoke-scrub");
    assert_eq!(pass.isolate(), Some(format!("--profile {SCRUB}").as_str()));
    assert_eq!(
        pass.files,
        lang_files(),
        "the scrub pass runs the language suite"
    );
    assert_rig_runs(&pass, "ELLE_RIG", "the scrub profile");
}

// A profile a pass names and nothing holds reads as no file at all: the rig
// refuses it, and the pass fails every file for a reason that is not the test.
#[test]
fn every_profile_a_pass_names_exists() {
    let mut named = 0;
    for target in suite_targets() {
        if let Some(path) = the_pass(&target)
            .isolate()
            .and_then(|flags| flags.strip_prefix("--profile "))
        {
            named += 1;
            assert!(
                crate::common::repo_root().join(path).exists(),
                "`make {target}` names the profile {path}, which does not exist"
            );
        }
    }
    assert!(
        named >= 3,
        "found {named} profile passes; the parse is broken"
    );
}

// The boot image's gate runs the language suite on an instance that hydrates
// its image instead of compiling the stdlib, and changes nothing else.
#[test]
fn smoke_boot_image_runs_the_language_suite_from_the_image() {
    let pass = the_pass("smoke-boot-image");
    assert_eq!(pass.files, lang_files());
    assert_eq!(
        pass.isolate(),
        Some(format!("--boot-image={}", make_expand("BOOT_IMAGE_DIR")).as_str()),
        "each file boots from the stored image"
    );
}

// `make smoke` is what the merge queue runs, and what a contributor runs
// before a push. `SMOKE_PASSES` names what it runs after `qa`, and the dry run
// shows it runs every pass of the default build. The counter-factual: a
// `SMOKE_PASSES` that `smoke` never hands to make reads right here and runs
// nothing.
#[test]
fn smoke_runs_every_pass_of_the_default_build() {
    let named = make_expand("SMOKE_PASSES");
    let named: Vec<&str> = named.split_whitespace().collect();
    let want = [
        "smoke-lang",
        "smoke-impl",
        "smoke-eager",
        "smoke-producers",
        "smoke-charge",
        "doctest",
        "embedding",
        "semver-check",
    ];
    for target in want {
        assert!(
            named.contains(&target),
            "`make smoke` does not run {target}: SMOKE_PASSES is {named:?}"
        );
    }
    let smoke: BTreeSet<String> = passes("smoke", &[])
        .into_iter()
        .map(|p| p.command)
        .collect();
    for target in &want[..5] {
        let pass = the_pass(target);
        assert!(
            smoke.contains(&pass.command),
            "`make smoke` does not run the pass of `make {target}`:\n  {}",
            pass.command
        );
    }
}
