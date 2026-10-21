// audited: 2026-10-21
//! The gates on the post-teardown residue: what one run may leave behind.
//!
//! docs/impl/region/rules.md
//! docs/impl/image/boot.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::primitives::module_init::StdlibSource;

/// The sweep must be *observable* and *idempotent*. The residual live-region
/// count is the standing oracle: it is the set of regions whose RC never reached
/// zero — and falls to zero as those are fixed, with no change to the teardown
/// itself. `tests/impl/oracle.lisp` measures the same residue as a per-op rate
/// on a running program; this counts what is left once the process is gone.
#[test]
fn process_teardown_is_observable_and_idempotent() {
    let mut rt = Runtime::new();

    // A representative program — define and use bindings, like any user file.
    let src = "(def squares (map (fn [x] (* x x)) (list 1 2 3)))";
    {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file(src, symbols, cctx, "<teardown-test>").expect("compiles");
        vm.execute_scheduled(&result, cctx).expect("runs");
    }

    let report = rt.teardown();

    // Observable: the sweep produced a census.
    eprintln!(
        "process teardown: {} regions remain (open leaks)",
        report.live_regions
    );

    // Idempotent: a second teardown is a safe no-op and never grows the residue
    // (it must not, for example, double-free or re-mint regions).
    let again = rt.teardown();
    assert!(
        again.live_regions <= report.live_regions,
        "second teardown grew the residue ({} -> {}) — not idempotent",
        report.live_regions,
        again.live_regions
    );
}

/// Teardown leaves no reference the region graph cannot explain
/// (docs/impl/region/rules.md). Every reference a surviving region carries comes
/// from another survivor's contents; a remainder is a claim held outside the
/// graph, and no release the region system reaches can ever balance it.
///
/// The counter-factual: the residue count alone cannot see this. A reference
/// cycle keeps its members alive with every reference explained, so the count
/// stays positive for a reason the graph shows. An unbalanced Rust-side claim
/// sits inside that number, indistinguishable from the cycle's shadow.
///
/// The run goes through `execute_scheduled`, the path every entry point takes,
/// so the scheduler wrapper's own allocations are inside the measurement. The
/// stdlib is compiled rather than read from the disk cache, so every region in
/// the residue was minted by this run and the verdict does not move with the
/// state of a cache file.
#[test]
fn teardown_leaves_no_unexplained_references() {
    for src in PROGRAMS {
        let pinned = pinned_after_teardown(Runtime::with_stdlib_cache(StdlibCache::Off), src);
        assert!(
            pinned.is_empty(),
            "{src}: regions pinned from outside the region graph: {pinned:#?}"
        );
    }
}

/// The programs both unexplained-reference gates run. Two, because the second
/// allocates through the stdlib and the first does not, and the claim is about
/// the runtime's own residue either way.
const PROGRAMS: [&str; 2] = [
    "(+ 1 2)",
    "(def squares (map (fn [x] (* x x)) (list 1 2 3)))",
];

/// The same property on the cache-hit boot path.
/// `teardown_leaves_no_unexplained_references` compiles the stdlib so its
/// verdict does not move with the state of a cache file. That is exactly what
/// stops it seeing this: a hit rebuilds the stdlib's closures, templates
/// and capture cells through an allocation context of its own, and the Rust
/// code that keeps them releases nothing.
///
/// The counter-factual: the residue count cannot see it either. The reload adds
/// one region to a residue of over a hundred, indistinguishable from the
/// reference cycle that explains the rest — while the in-degree reading names it
/// outright, rc=1 against no in-edge at all.
#[test]
fn a_cache_hit_leaves_no_unexplained_references() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = StdlibCache::Dir(dir.path().to_path_buf());
    // The first runtime meets an empty directory, so it compiles and stores.
    drop(Runtime::with_stdlib_cache(cache.clone()));
    for src in PROGRAMS {
        let rt = Runtime::with_stdlib_cache(cache.clone());
        assert_eq!(
            rt.stdlib_source(),
            StdlibSource::Cache,
            "this runtime must load the stdlib the seeding one stored; a miss \
             yields a working runtime too, so it would pass the assertion below \
             without ever exercising the reload"
        );
        let pinned = pinned_after_teardown(rt, src);
        assert!(
            pinned.is_empty(),
            "{src}: regions pinned from outside the region graph: {pinned:#?}"
        );
    }
}

/// The same property on the image-boot path (docs/impl/image/boot.md). A whole
/// boot graph arrives as one hydrated region registered as one process root,
/// so the sweep either releases all of it or none of it.
///
/// The counter-factual is a per-root registration. Registering the core
/// exports, the stdlib exports and the macro table separately takes three
/// references against a region that was minted with one. The residue count
/// alone cannot tell that from the reference cycle it sits inside, while the
/// in-degree reading names it: rc above the in-edges it can explain.
#[test]
fn an_image_boot_leaves_no_unexplained_references() {
    let dir = tempfile::tempdir().expect("tempdir");
    let caches = || elle::runtime::BootCaches {
        stdlib: StdlibCache::Dir(dir.path().join("stdlib")),
        image: elle::image::boot::BootImage::Dir(dir.path().join("boot")),
    };
    // The first runtime meets an empty directory, so it compiles and stores.
    drop(Runtime::with_caches(caches()));
    for src in PROGRAMS {
        let rt = Runtime::with_caches(caches());
        assert_eq!(
            rt.boot_source(),
            elle::image::boot::BootSource::Image,
            "this runtime must hydrate the image the seeding one stored; a miss \
             yields a working runtime too, so it would pass the assertion below \
             without ever exercising an image boot"
        );
        let pinned = pinned_after_teardown(rt, src);
        assert!(
            pinned.is_empty(),
            "{src}: regions pinned from outside the region graph: {pinned:#?}"
        );
    }
}

/// A completed run leaves NOTHING (docs/impl/region/rules.md). Zero is the claim, not a
/// target the count may approach: the sweep is RC-driven, so a survivor is a reference
/// that never reached zero, and the number IS the remaining work.
///
/// The counter-factual is `teardown_leaves_no_unexplained_references` beside it,
/// which this does not subsume in either direction. That one sees a claim held
/// outside the region graph and is blind to a cycle. Every reference in a cycle
/// is explained by another survivor's contents, so it reads clean while the
/// cycle stands. This one sees the cycle and cannot say what pins it. A cycle in the
/// scheduler wrapper can hold every other survivor while that gate reads clean.
///
/// The run goes through `execute_scheduled`, the path every entry point takes, so
/// the scheduler wrapper is inside the measurement. The stdlib is compiled rather
/// than read from the disk cache, so every region in the residue was minted by
/// this run.
#[test]
fn teardown_leaves_no_residue() {
    for src in PROGRAMS {
        assert_eq!(
            residue_after_teardown(
                Runtime::with_stdlib_cache(StdlibCache::Off),
                src,
                HandOff::Root
            ),
            0,
            "{src}: regions survived teardown — every one is a reference the run \
             never dropped",
        );
    }
}

/// How the host gives back the one owning reference a run hands it with the program
/// value (docs/impl/region/rules.md). A host that does neither reads the residue of its
/// own hand-off rather than the run's.
#[derive(Clone, Copy)]
pub(super) enum HandOff {
    /// Register the value as a process root, so the sweep consumes it.
    Root,
    /// Release the value where the host stops reading it — the branch the
    /// `elle` binary's run path takes.
    Release,
}

/// Run `src` on `rt`, give the program value back as `handoff` says, tear the
/// runtime down, and answer how many regions survived — printing the census
/// whenever any did.
///
/// The stdlib is compiled rather than read from the disk cache, so every region
/// in the residue was minted by this run and the verdict does not move with the
/// state of a cache file. The run goes through `execute_scheduled`, the path
/// every entry point takes, so the scheduler wrapper is inside the measurement.
pub(super) fn residue_after_teardown(mut rt: Runtime, src: &str, handoff: HandOff) -> usize {
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file(src, symbols, cctx, "<residue>").expect("compiles");
        vm.execute_scheduled(&result, cctx).expect("runs")
    };
    match handoff {
        HandOff::Root => elle::value::arena::register_process_root(
            rt.heap(),
            value,
            elle::value::arena::RootRef::Take,
        ),
        HandOff::Release => elle::value::arena::release_program_value(rt.heap(), value),
    }
    let report = rt.teardown();
    if report.live_regions != 0 {
        report_census(rt.heap(), &report);
    }
    report.live_regions
}

/// A run whose program value is a HEAP value leaves nothing behind when the
/// host releases that value instead of rooting it (docs/impl/region/rules.md).
///
/// The counter-factual is the registration every other residue gate makes. Those route
/// the returned reference into the process-root registry, so the sweep consumes
/// it and the residue reads zero whether or not the release funnel works at
/// all. This one takes the other branch of the same obligation — release now,
/// register nothing — which is the branch the `elle` binary's run path takes.
///
/// Neither program in `PROGRAMS` can see it: both answer with an immediate,
/// which occupies no region, so their hand-off releases nothing whichever
/// branch it takes. Each shape here answers with a heap value, and is measured
/// beside a program that allocates the same value and discards it — so the two
/// readings differ in the hand-off alone.
#[test]
fn a_released_heap_program_value_leaves_no_residue() {
    for answer in ["(list 1 2 3)", "\"str\"", "[1 2 3]", "{:a 1}", "(fn [x] x)"] {
        for src in [answer.to_string(), format!("(begin {answer} nil)")] {
            assert_eq!(
                residue_after_teardown(
                    Runtime::with_stdlib_cache(StdlibCache::Off),
                    &src,
                    HandOff::Release
                ),
                0,
                "{src}: regions survived teardown after the host released the \
                 program value",
            );
        }
    }
}
