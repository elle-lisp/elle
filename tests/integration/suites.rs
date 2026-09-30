// audited: 2026-09-29
// What each suite target runs: the language suite with no flag, the
// implementation suite on the rig, and each profile's files.
//
// docs/testing.md
// rig/overview.md
//
// Every assertion reads `make --dry-run`, which prints each command a target
// will run with every variable resolved. A target that names the wrong files or
// passes a flag the language suite must never see still runs, still records a
// verdict per file, and still gates green, so the recipe is checked here.

use crate::common::{make_dry_run, make_dry_run_with, make_expand, makefile, suite};
use std::collections::BTreeSet;

const EAGER: &str = "tests/impl/profiles/jit-eager.toml";
const SCRUB: &str = "tests/impl/profiles/scrub.toml";
const WASM_FULL: &str = "tests/impl/profiles/wasm-full.toml";
const ACCEPTANCE: &str = "tests/runner/acceptance.lisp";

/// One `elle test` batch pass: the files it deals out, and the command each
/// batch runs.
struct Pass {
    files: BTreeSet<String>,
    command: String,
}

impl Pass {
    /// The child flags the pass gives each file: what `--isolate` quotes.
    fn isolate(&self) -> &str {
        let (_, rest) = self
            .command
            .split_once("--isolate '")
            .unwrap_or_else(|| panic!("a suite pass runs each file as its own child:\n{}", self.command));
        rest.split_once('\'').map(|(flags, _)| flags).unwrap_or("")
    }

    /// The program each child runs under, when the pass names one.
    fn host(&self) -> Option<&str> {
        let (_, rest) = self.command.split_once("--host ")?;
        rest.split_whitespace().next()
    }
}

/// Every suite pass `make TARGET VARS…` will run, in order.
fn passes(target: &str, vars: &[&str]) -> Vec<Pass> {
    let recipe = make_dry_run_with(target, vars)
        .unwrap_or_else(|| panic!("`make --dry-run {target}` did not run"));
    let passes: Vec<Pass> = recipe
        .lines()
        .filter(|line| line.contains(" test ") && line.contains("xargs"))
        .map(|line| Pass {
            files: line
                .split_whitespace()
                .filter(|w| w.starts_with("tests/") && w.ends_with(".lisp"))
                .map(str::to_string)
                .collect(),
            command: line.to_string(),
        })
        .collect();
    assert!(
        !passes.is_empty(),
        "`make {target}` runs no suite pass:\n{recipe}"
    );
    passes
}

fn set(paths: Vec<String>) -> BTreeSet<String> {
    paths.into_iter().collect()
}

fn lang() -> BTreeSet<String> {
    set(suite("tests/lang"))
}

fn implementation() -> BTreeSet<String> {
    set(suite("tests/impl"))
}

/// Assert that `pass` runs the language suite as a user runs it: every file,
/// on the build itself, with no flag.
fn assert_plain_language_pass(target: &str, pass: &Pass) {
    assert_eq!(
        pass.files,
        lang(),
        "`make {target}` does not run exactly the language suite"
    );
    assert_eq!(
        pass.isolate(),
        "",
        "`make {target}` hands every language file a flag, so the suite runs a \
         runtime no user runs"
    );
    assert_eq!(
        pass.host(),
        None,
        "`make {target}` runs the language suite on another program than the build"
    );
}

// The language suite runs the user build as shipped. Each file is its own
// process with no flag, which is how a user runs a program.
//
// The counter-factual: a pass that isolated each file under `--trace=scrub`, or
// under the rig, reports a green language suite for a runtime no user runs.
#[test]
fn smoke_lang_runs_every_language_file_with_no_flag() {
    let passes = passes("smoke-lang", &[]);
    assert_eq!(passes.len(), 1, "`make smoke-lang` is one pass");
    assert_plain_language_pass("smoke-lang", &passes[0]);
}

// The implementation suite runs on the rig, so each file's sidecar sets its
// mode. The runner's acceptance test rides the same pass: it drives `elle test`
// itself, and it needs the store the pass records into.
//
// The counter-factual: run the implementation suite under `elle`, and every
// sidecar goes unread. The guardfree files run with the oracle disarmed and
// pass.
#[test]
fn smoke_impl_runs_the_implementation_suite_on_the_rig() {
    let passes = passes("smoke-impl", &[]);
    let base = &passes[0];
    let mut want = implementation();
    want.insert(ACCEPTANCE.to_string());
    assert_eq!(
        base.files, want,
        "the first pass of `make smoke-impl` is the implementation suite and \
         the runner's acceptance test"
    );
    assert_eq!(base.isolate(), "", "each sidecar alone sets its file's mode");
    assert_eq!(
        base.host(),
        Some(make_expand("ELLE_RIG").as_str()),
        "the implementation suite runs on the rig"
    );
}

// The eager profile runs both suites with every function compiled on its first
// call. A profile's `jit` replaces a sidecar's, so the pinned files meet the
// eager tier here too.
#[test]
fn smoke_impl_runs_both_suites_under_the_eager_profile() {
    let passes = passes("smoke-impl", &[]);
    let eager: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.isolate() == format!("--profile {EAGER}"))
        .collect();
    assert_eq!(eager.len(), 1, "one pass runs the eager profile");
    let want: BTreeSet<String> = lang().union(&implementation()).cloned().collect();
    assert_eq!(eager[0].files, want, "the eager profile runs both suites");
    assert_eq!(
        eager[0].host(),
        Some(make_expand("ELLE_RIG").as_str()),
        "a profile is a rig setting"
    );
}

// `IMPL_PROFILES` names further profiles for the language suite. The macOS job
// sets it to the scrub profile (tests/integration/workflows.rs).
#[test]
fn a_named_profile_runs_the_language_suite_on_the_rig() {
    let var = format!("IMPL_PROFILES={SCRUB}");
    let passes = passes("smoke-impl", &[&var]);
    let scrub: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.isolate() == format!("--profile {SCRUB}"))
        .collect();
    assert_eq!(scrub.len(), 1, "one pass runs the named profile");
    assert_eq!(scrub[0].files, lang(), "a named profile runs the language suite");
    assert_eq!(scrub[0].host(), Some(make_expand("ELLE_RIG").as_str()));
}

/// Whether `WASM_SKIP` leaves `path` out of the wasm build's passes. Each
/// pattern is a substring of a path, the way `grep -v -e` reads it.
fn wasm_skipped(path: &str) -> bool {
    make_expand("WASM_SKIP")
        .split_whitespace()
        .filter(|word| *word != "-e")
        .any(|pattern| path.contains(pattern))
}

// The wasm build's rig runs the implementation suite twice. The first pass reads
// each file's sidecar, as every rig does; the tiered backend's files run there,
// because each forces its closures onto the tier itself.
//
// The counter-factual: a `smoke-wasm` with no such pass leaves
// tests/impl/wasm-tier-error-signal.lisp on no wasm build at all, since the
// full-module pass skips it.
#[test]
fn smoke_wasm_runs_the_implementation_suite_on_the_wasm_rig() {
    let passes = passes("smoke-wasm", &[]);
    let base: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.host().is_some() && p.isolate().is_empty())
        .collect();
    assert_eq!(base.len(), 1, "one wasm rig pass reads each file's sidecar");
    assert_eq!(base[0].host(), Some(make_expand("ELLE_RIG").as_str()));
    assert_eq!(
        base[0].files,
        implementation(),
        "the wasm rig's sidecar pass runs the whole implementation suite"
    );
}

// The second wasm rig pass compiles each file whole to one module, less the
// files `WASM_SKIP` names. The three files the test names pin invariants only
// the full-module tier has to uphold (docs/impl/wasm.md, src/wasm/mod.rs,
// src/wasm/tests/toplevel.rs).
//
// The counter-factual: a `smoke-wasm` that runs only the language suite on the
// full-module tier leaves every one of those files off the tier it pins, and
// nothing reports it.
#[test]
fn smoke_wasm_runs_the_implementation_suite_under_the_wasm_full_profile() {
    let passes = passes("smoke-wasm", &[]);
    let full: Vec<&Pass> = passes
        .iter()
        .filter(|p| p.isolate() == format!("--profile {WASM_FULL}"))
        .collect();
    assert_eq!(full.len(), 1, "one wasm rig pass compiles each file whole");
    let pass = full[0];
    assert_eq!(pass.host(), Some(make_expand("ELLE_RIG").as_str()));
    let want: BTreeSet<String> = implementation()
        .into_iter()
        .filter(|path| !wasm_skipped(path))
        .collect();
    assert_eq!(
        pass.files, want,
        "the wasm-full pass runs every implementation file `WASM_SKIP` keeps"
    );
    for pin in [
        "tests/impl/region-capture-cell-loop-uaf.lisp",
        "tests/impl/region-termination-sweep.lisp",
        "tests/impl/region-eval-quoted-data-leak.lisp",
    ] {
        assert!(pass.files.contains(pin), "the wasm-full pass leaves out {pin}");
    }
}

// A profile a pass names and nothing holds reads as no file at all: the rig
// refuses it, and the pass fails every file for a reason that is not the test.
#[test]
fn every_profile_a_pass_names_exists() {
    let named = passes("smoke-impl", &[&format!("IMPL_PROFILES={SCRUB}")]);
    for pass in named.into_iter().chain(passes("smoke-wasm", &[])) {
        if let Some(path) = pass.isolate().strip_prefix("--profile ") {
            assert!(
                crate::common::repo_root().join(path).exists(),
                "a pass names the profile {path}, which does not exist"
            );
        }
    }
}

// Each build is an implementation, and each runs the language suite as it is.
// The counter-factual for each: a variant target that ran the suite on the
// default build, or passed the old tier flag instead of building without the
// tier, tests the default build twice and the variant never.
#[test]
fn each_variant_runs_the_language_suite_on_its_own_build() {
    for (target, build) in [
        ("smoke-nojit", "elle-nojit"),
        ("smoke-mlir", "elle-mlir"),
        ("smoke-pool", "elle-pool"),
    ] {
        let recipe = make_dry_run(target).expect("dry run");
        let build_recipe = make_dry_run(build).expect("dry run");
        let build_line = build_recipe
            .lines()
            .find(|l| l.contains("cargo build"))
            .unwrap_or_else(|| panic!("`make {build}` builds nothing"));
        assert!(
            recipe.contains(build_line),
            "`make {target}` does not build {build} before its passes"
        );
        let passes = passes(target, &[]);
        assert_plain_language_pass(target, &passes[0]);
    }
}

// The build with no features cannot compile a file that calls an `ffi/`
// primitive, so its pass leaves those out and runs every other language file.
// Nor can it run the runner, whose store reaches SQLite through FFI, so the
// runner is the default build and each child is the no-features binary.
//
// The counter-factual for the host: with none, the runner is whatever `ELLE`
// names. If that is the no-features binary the pass dies importing its store,
// and if it is the default build the pass tests the default build twice.
#[test]
fn smoke_noffi_runs_the_language_suite_without_the_ffi_files() {
    let passes = passes("smoke-noffi", &[]);
    assert_eq!(passes.len(), 1);
    let pass = &passes[0];
    assert!(pass.files.is_subset(&lang()));
    let skipped = lang().len() - pass.files.len();
    assert!(
        (1..=20).contains(&skipped),
        "the no-features pass leaves out {skipped} language files; the FFI \
         files are a handful"
    );
    assert_eq!(pass.isolate(), "");
    let noffi = make_expand("ELLE_NOFFI");
    assert_ne!(
        noffi,
        make_expand("ELLE"),
        "the no-features binary is a file of its own, beside the runner's build"
    );
    assert_eq!(
        pass.host(),
        Some(noffi.as_str()),
        "each language file runs as a child of the no-features build"
    );
    let build = make_dry_run("elle-noffi").expect("dry run");
    assert!(
        build.contains(&noffi),
        "`make elle-noffi` leaves nothing at {noffi}, where the pass looks:\n{build}"
    );
}

// The thread-pool build's rig runs the implementation suite too: some of its
// files count the pool's worker threads, which exist on no other build.
#[test]
fn smoke_pool_runs_the_implementation_suite_on_the_pool_rig() {
    let passes = passes("smoke-pool", &[]);
    let rig: Vec<&Pass> = passes.iter().filter(|p| p.host().is_some()).collect();
    assert_eq!(rig.len(), 1, "one pool pass runs on the rig");
    assert_eq!(rig[0].files, implementation());
    assert_eq!(rig[0].isolate(), "");
}

/// The `cargo build` line `make TARGET` runs.
fn build_line(target: &str) -> String {
    let recipe = make_dry_run(target).unwrap_or_else(|| panic!("`make {target}` did not run"));
    recipe
        .lines()
        .find(|l| l.contains("cargo build"))
        .unwrap_or_else(|| panic!("`make {target}` builds nothing:\n{recipe}"))
        .to_string()
}

// A build is its features. The counter-factual: `elle-nojit` built with the
// default features compiles the JIT back in, and the No-JIT job runs the
// default build under another name.
#[test]
fn each_variant_build_carries_the_features_its_name_promises() {
    for (target, features) in [
        ("elle-nojit", "--no-default-features --features ffi,uring"),
        ("elle-pool", "--no-default-features --features jit,ffi"),
        ("elle-mlir", "--features mlir"),
        ("elle-wasm", "--features wasm"),
    ] {
        let line = build_line(target);
        assert!(
            line.contains(features),
            "`make {target}` builds without `{features}`:\n  {line}"
        );
    }
    let noffi = build_line("elle-noffi");
    assert!(
        noffi.contains("--no-default-features") && !noffi.contains("--features"),
        "`make elle-noffi` builds with a feature:\n  {noffi}"
    );
    assert!(
        build_line("elle-pool").contains("elle-rig"),
        "the pool build's rig runs the implementation suite, so `make elle-pool` \
         builds it"
    );
    assert!(build_line("elle-rig").contains("-p elle-rig"));
    assert!(
        build_line("elle-wasm").contains("-p elle-rig"),
        "the wasm build's rig runs the implementation suite, so `make elle-wasm` \
         builds it"
    );
}

// The boot image's gate runs the language suite on an instance that hydrates
// its image instead of compiling the stdlib, and changes nothing else.
#[test]
fn smoke_boot_image_runs_the_language_suite_from_the_image() {
    let passes = passes("smoke-boot-image", &[]);
    assert_eq!(passes.len(), 1);
    assert_eq!(passes[0].files, lang());
    assert_eq!(
        passes[0].isolate(),
        format!("--boot-image={}", make_expand("BOOT_IMAGE_DIR")),
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
    for want in ["smoke-lang", "smoke-impl", "doctest", "embedding", "semver-check"] {
        assert!(deps.contains(&want), "`make smoke` does not run {want}: {line}");
    }
}
