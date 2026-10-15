// audited: 2026-10-15
// Each variant build's targets: the binaries they build, their features, and
// the suite pass each runs on them.
//
// docs/testing.md
// bins/overview.md
// rig/overview.md
//
// Every assertion reads `make --dry-run`, as suites.rs does for the default
// build. A variant target that runs its suites on the default build still runs,
// still records a verdict per file, and still gates green, so the recipe is
// checked here.

use crate::common::{
    assert_plain_language_pass, assert_producer_pass, assert_rig_runs, impl_files,
    isolated_impl_files, lang_files, make_dry_run, make_expand, the_pass,
};
use std::collections::BTreeSet;

const WASM_FULL: &str = "tests/impl/profiles/wasm-full.toml";

/// Whether `WASM_SKIP` leaves `path` out of the wasm build's passes. Each
/// pattern is a substring of a path, the way `grep -v -e` reads it.
fn wasm_skipped(path: &str) -> bool {
    make_expand("WASM_SKIP")
        .split_whitespace()
        .filter(|word| *word != "-e")
        .any(|pattern| path.contains(pattern))
}

// The wasm build runs the language suite as every build does: inside the
// runner of the build itself, `elle-wasm`. The counter-factual: a pass that runs
// `elle` runs the language suite on the default build, and the wasm build runs
// it nowhere.
#[test]
fn smoke_wasm_runs_the_language_suite_in_process_on_elle_wasm() {
    assert_plain_language_pass("smoke-wasm", &the_pass("smoke-wasm"), "ELLE_WASM");
}

// The wasm build's rig runs the implementation suite under each file's
// sidecar, as every rig does; the tiered backend's files run there, because
// each forces its closures onto the tier itself.
//
// The counter-factual: a wasm build with no such pass leaves
// tests/impl/wasm-tier.lisp on no wasm build at all, since the
// full-module pass skips it.
#[test]
fn smoke_wasm_impl_runs_the_implementation_suite_on_the_wasm_rig() {
    let pass = the_pass("smoke-wasm-impl");
    assert_eq!(
        pass.isolate(),
        Some(""),
        "each file's sidecar sets its mode"
    );
    assert_eq!(pass.host(), Some(make_expand("ELLE_RIG_WASM").as_str()));
    assert_eq!(pass.runner(), make_expand("ELLE_WASM"));
    assert_eq!(
        pass.files,
        impl_files(),
        "the wasm rig's sidecar pass runs the whole implementation suite"
    );
}

// The second wasm rig pass compiles each file of both suites whole to one
// module, less the files `WASM_SKIP` names. The four files the test names pin
// invariants only the full-module tier has to uphold (docs/impl/wasm.md,
// src/wasm/mod.rs, src/wasm/tests/toplevel.rs).
//
// The counter-factual: a wasm build that runs the language suite only inside
// the runner leaves tests/lang/posix.lisp off the full-module tier, where every
// io-backend strands to the heap's teardown, and nothing reports it.
#[test]
fn smoke_wasm_full_runs_both_suites_under_the_wasm_full_profile() {
    let pass = the_pass("smoke-wasm-full");
    assert_eq!(
        pass.isolate(),
        Some(format!("--profile {WASM_FULL}").as_str()),
        "the pass compiles each file whole"
    );
    assert_eq!(pass.host(), Some(make_expand("ELLE_RIG_WASM").as_str()));
    assert_eq!(pass.runner(), make_expand("ELLE_WASM"));
    let want: BTreeSet<String> = lang_files()
        .union(&impl_files())
        .filter(|path| !wasm_skipped(path))
        .cloned()
        .collect();
    assert_eq!(
        pass.files, want,
        "the wasm-full pass runs every file of both suites `WASM_SKIP` keeps"
    );
    for pin in [
        "tests/lang/posix.lisp",
        "tests/impl/region-capture-cell-loop-uaf.lisp",
        "tests/impl/region-termination-sweep.lisp",
        "tests/impl/region-eval-quoted-data-leak.lisp",
    ] {
        assert!(
            pass.files.contains(pin),
            "the wasm-full pass leaves out {pin}"
        );
    }
}

// Each build is an implementation, and each runs the language suite as it is.
// The counter-factual for each: a variant target that ran the suite on the
// default build, or passed the old tier flag instead of building without the
// tier, tests the default build twice and the variant never.
#[test]
fn each_variant_runs_the_language_suite_on_its_own_build() {
    for (target, runner) in [
        ("smoke-nojit", "ELLE"),
        ("smoke-mlir", "ELLE_MLIR"),
        ("smoke-pool", "ELLE"),
    ] {
        assert_plain_language_pass(target, &the_pass(target), runner);
    }
}

// Each suite target of a variant builds that variant before its pass, so a
// target run as a CI step of its own runs on the build its name promises. The
// counter-factual: a `smoke-pool-impl` that leaned on `smoke-pool` having
// built first runs on whatever build `target/` last held.
#[test]
fn each_variant_target_builds_its_variant_first() {
    for (build, targets) in [
        ("elle-nojit", &["smoke-nojit"][..]),
        (
            "elle-mlir",
            &["smoke-mlir", "smoke-mlir-impl", "smoke-mlir-producers"][..],
        ),
        (
            "elle-pool",
            &["smoke-pool", "smoke-pool-impl", "smoke-pool-producers"][..],
        ),
        (
            "elle-wasm",
            &["smoke-wasm", "smoke-wasm-impl", "smoke-wasm-full"][..],
        ),
    ] {
        for target in targets {
            let recipe =
                make_dry_run(target).unwrap_or_else(|| panic!("`make {target}` did not run"));
            assert!(
                recipe.contains(&build_line(build)),
                "`make {target}` does not build {build} before its pass"
            );
        }
    }
}

// The MLIR build's rig is the one rig that carries the MLIR tier, so the
// implementation suite's MLIR files run there and nowhere else. The
// counter-factual: an MLIR build that runs the language suite alone runs
// tests/impl/mlir.lisp on no build that carries the tier it tests.
#[test]
fn smoke_mlir_impl_runs_the_implementation_suite_on_the_mlir_rig() {
    let pass = the_pass("smoke-mlir-impl");
    assert_rig_runs(
        &pass,
        "ELLE_RIG_MLIR",
        "the MLIR build's implementation suite",
    );
    assert_eq!(pass.files, isolated_impl_files());
    assert_eq!(
        pass.isolate(),
        Some(""),
        "each sidecar sets its file's mode"
    );
    let all = [pass, the_pass("smoke-mlir-producers")];
    assert_producer_pass("the MLIR build's rig targets", &all, "ELLE_RIG_MLIR");
}

// A variant's binaries land beside the default build's and never over them
// (bins/overview.md). The counter-factual: `make elle-mlir` built as `-p elle
// --features mlir` wrote `elle` itself, and every later target that ran `elle`
// ran the MLIR build.
#[test]
fn a_variant_binary_lands_beside_elle_and_never_over_it() {
    let out = make_expand("CARGO_OUT");
    for (variant, program, rig) in [
        ("wasm", "ELLE_WASM", "ELLE_RIG_WASM"),
        ("mlir", "ELLE_MLIR", "ELLE_RIG_MLIR"),
    ] {
        assert_eq!(make_expand(program), format!("{out}/elle-{variant}"));
        assert_eq!(make_expand(rig), format!("{out}/elle-rig-{variant}"));
        let line = build_line(&format!("elle-{variant}"));
        for want in [
            format!("--manifest-path bins/{variant}/Cargo.toml"),
            "--target-dir target".to_string(),
        ] {
            assert!(
                line.contains(&want),
                "`make elle-{variant}` builds without `{want}`, so its binaries \
                 are not the ones {program} names:\n  {line}"
            );
        }
        assert!(
            !line.contains("-p elle "),
            "`make elle-{variant}` builds the root package, whose binary is \
             `elle`:\n  {line}"
        );
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
    let pass = &the_pass("smoke-noffi");
    assert!(pass.files.is_subset(&lang_files()));
    let skipped = lang_files().len() - pass.files.len();
    assert!(
        (1..=20).contains(&skipped),
        "the no-features pass leaves out {skipped} language files; the FFI \
         files are a handful"
    );
    assert_eq!(
        pass.isolate(),
        Some(""),
        "each no-features child runs with no flag"
    );
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
fn smoke_pool_impl_runs_the_implementation_suite_on_the_pool_rig() {
    let pass = the_pass("smoke-pool-impl");
    assert_rig_runs(&pass, "ELLE_RIG", "the pool build's implementation suite");
    assert_eq!(pass.files, isolated_impl_files());
    assert_eq!(pass.isolate(), Some(""));
    let all = [pass, the_pass("smoke-pool-producers")];
    assert_producer_pass("the pool build's rig targets", &all, "ELLE_RIG");
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
}
