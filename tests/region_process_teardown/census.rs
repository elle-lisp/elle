// audited: 2026-09-20
// What one run leaves behind: the gates on the post-teardown residue, and the
// per-shape censuses that name the classes it is made of.
// docs/impl/region/rules.md
// docs/impl/region/diagnostics.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::primitives::module_init::StdlibSource;

/// The sweep must be *observable* and *idempotent*. The residual live-region
/// count is the standing oracle: it is the set of regions whose RC never reached
/// zero — and falls to zero as those are fixed, with no change to the teardown
/// itself. `tests/elle/oracle.lisp` measures the same residue as a per-op rate
/// on a running program; this counts what is left once the process is gone.
#[test]
fn process_teardown_is_observable_and_idempotent() {
    let mut rt = Runtime::new();

    // A representative program — define and use bindings, like any user file.
    let src = "(def squares (map (fn [x] (* x x)) (list 1 2 3)))";
    {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file(src, symbols, cctx, "<teardown-test>").expect("compiles");
        vm.execute_scheduled(&result.bytecode, cctx).expect("runs");
    }

    let report = rt.teardown();

    // Observable: the sweep produced a census.
    eprintln!(
        "process teardown: {} regions remain (open leaks)",
        report.live_regions
    );

    // Idempotent: a second teardown is a safe no-op and never grows the residue
    // (it must not, e.g., double-free or re-mint regions).
    let again = rt.teardown();
    assert!(
        again.live_regions <= report.live_regions,
        "second teardown grew the residue ({} -> {}) — not idempotent",
        report.live_regions,
        again.live_regions
    );
}

/// Teardown leaves no reference the region graph cannot explain
/// (docs/impl/region/rules.md § "Teardown — every region frees"). Every
/// reference a surviving region carries comes from another survivor's contents;
/// a remainder is a claim held outside the graph, and no release the region
/// system reaches can ever balance it.
///
/// The counter-factual: the residue count alone cannot see this. A reference
/// cycle keeps its members alive with every reference explained, so the count
/// stays positive for a reason the graph shows, and an unbalanced Rust-side
/// claim sits inside that number indistinguishable from the cycle's shadow.
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
/// verdict does not move with the state of a cache file, and that is exactly
/// what stops it seeing this: a hit rebuilds the stdlib's closures, templates
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

/// A completed run leaves NOTHING (docs/impl/region/rules.md § "Teardown — every
/// region frees", property 2). Zero is the claim, not a target the count may
/// approach: the sweep is RC-driven, so a survivor is a reference that never
/// reached zero, and the number IS the remaining work.
///
/// The counter-factual is `teardown_leaves_no_unexplained_references` beside it,
/// which this does not subsume in either direction. That one sees a claim held
/// outside the region graph and is blind to a cycle — every reference in a cycle
/// is explained by another survivor's contents, so it reads clean while the cycle
/// stands. This one sees the cycle and cannot say what pins it. The residue of
/// every program ever run was one 42-region cycle holding 100% of the rest, and
/// nothing failed on it (elle-lisp/elle#1081).
///
/// The run goes through `execute_scheduled`, the path every entry point takes, so
/// the scheduler wrapper is inside the measurement — which is where that cycle
/// lived. The stdlib is compiled rather than read from the disk cache, so every
/// region in the residue was minted by this run.
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

/// How the host gives back the one owning reference a run hands it with the
/// program value (docs/impl/region/rules.md § "The program value is the host's
/// to release"). A host that does neither reads the residue of its own
/// hand-off rather than the run's.
#[derive(Clone, Copy)]
enum HandOff {
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
fn residue_after_teardown(mut rt: Runtime, src: &str, handoff: HandOff) -> usize {
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file(src, symbols, cctx, "<residue>").expect("compiles");
        vm.execute_scheduled(&result.bytecode, cctx).expect("runs")
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

/// A run that spawns a child leaves nothing either. The `subprocess` value is
/// built when the spawn finishes rather than by the call that asked for it, so
/// the region it lives in is one the completion owns and hands over
/// (docs/impl/io-inflight.md § "A completion owns what it builds").
///
/// The counter-factual is `teardown_leaves_no_residue` beside it, which cannot
/// see this at all: neither of its programs reaches the io backend, so no
/// completion in that run ever builds a value. A completion that keeps what it
/// built leaves one region per child, linear in the number of children, and
/// every gate in this file reads clean while it does.
///
/// The child is `/bin/sh -c :`, an absolute path so no `PATH` decides whether
/// this test measures a spawn or an `exec-error`. Both answers are built the
/// same way, so the error would read as a pass.
#[test]
fn a_run_that_spawns_a_child_leaves_no_residue() {
    let src = "(let [p (subprocess/exec \"/bin/sh\" [\"-c\" \":\"])] \
               (subprocess/wait p))";
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run that bounds work with a deadline leaves nothing either. `ev/timeout`
/// spawns the body and a timer, and aborts whichever lost — so a program that
/// calls it once ends with one fiber aborted through the scheduler and one I/O
/// operation cancelled.
///
/// Three holders keep an aborted fiber, and all three have to let go for this
/// to read zero: the loop's completion record and its mark, both keyed by the
/// fiber (docs/scheduler.md § "Completion records"); the runnable queue, which
/// a completed fiber leaves on the same rule; and the cancelled operation's own
/// entry, which retains the fiber it would have answered
/// (docs/impl/io-inflight.md § "A cancelled operation reads nothing again").
///
/// The counter-factual is `teardown_leaves_no_residue` beside it, which reaches
/// none of this: neither of its programs spawns a fiber, so no fiber is ever
/// completed, aborted or cancelled for. Each holder is worth the fiber, its
/// closure and the payload the abort delivered, and every gate in this file
/// reads clean while they hold.
///
/// The body wins on purpose. A timer of 30 seconds cannot fire inside a test,
/// so the abort always lands on the timer and the run never waits for one —
/// and a body that wins at once is also what leaves the loop no chance to reap
/// the cancellation it just issued.
#[test]
fn a_run_that_times_out_leaves_no_residue() {
    let src = "(ev/timeout 30 (fn [] 1))";
    assert_eq!(
        residue_after_teardown(Runtime::with_stdlib_cache(StdlibCache::Off), src),
        0,
        "{src}: regions survived teardown",
    );
}

/// The same claim for the other shape a completion builds: bytes whose length
/// nothing could reserve ahead of the read.
///
/// `port/read` and `port/read-line` answer from a buffer the call pre-allocated
/// and reach none of this, so a read alone is no evidence. `read-all` has no
/// count to reserve against, so its answer is born when the stream ends — which
/// is what makes it the discriminator for the same defect the spawn shows.
#[test]
fn a_run_that_reads_a_whole_file_leaves_no_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("residue-read-all");
    std::fs::write(&path, b"census").expect("write");
    let src = format!(
        "(let [p (port/open \"{}\" :read) s (port/read-all p)] (port/close p) (length s))",
        path.display()
    );
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            &src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// One call whose completions build more than one answer: `subprocess/system`
/// spawns the child, reads both pipes to the end, and waits
/// (docs/impl/io-inflight.md § "A completion owns what it builds").
///
/// It reaches what the two cases above do not. The `read-all` there reads a file
/// its own call opened; this one reads a pipe, through a port a completion built
/// and handed over. And neither of those can price a call that builds twice:
/// with the handover removed, the spawn above leaves 1 region behind and the
/// file read-all 3, where this leaves 5.
///
/// The child prints, so the `read-all` on its stdout answers with bytes rather
/// than the `nil` an empty pipe gives — the shape a caller reads. It is named by
/// an absolute path for the reason the spawn case gives: an `exec-error` is
/// built the same way, so it would read as a pass.
#[test]
fn a_run_that_captures_what_a_child_wrote_leaves_no_residue() {
    let src = "(get (subprocess/system \"/bin/sh\" [\"-c\" \"echo census\"]) :exit)";
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run whose program value is a HEAP value leaves nothing behind when the
/// host releases that value instead of rooting it
/// (docs/impl/region/rules.md § "The program value is the host's to release").
///
/// The counter-factual is the registration every gate above makes. Those route
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

/// Diagnostic, not a gate: dump the post-teardown residue (id, rc, objs, tags)
/// for offline aggregation. The residue census names the dominant leak classes
/// of the standing live-region count — run with
/// `cargo test --test region_process_teardown teardown_residue_census -- --ignored --nocapture`.
#[test]
#[ignore = "diagnostic: prints the teardown residue for leak-class aggregation"]
fn teardown_residue_census() {
    census(Runtime::new(), "(+ 1 2)");
}

/// Same census without the stdlib: isolates whether the leak classes come from
/// compiling/running source at all (they appear here) or from the stdlib load
/// (they only appear in `teardown_residue_census`).
#[test]
#[ignore = "diagnostic: residue census without stdlib"]
fn teardown_residue_census_no_stdlib() {
    census(Runtime::without_stdlib(), "(%add 1 2)");
}

/// Smallest possible run: compile and execute a bare literal, no stdlib.
/// Any residue here is the per-compilation floor every eval pays.
#[test]
#[ignore = "diagnostic: residue census for a bare literal, no stdlib"]
fn teardown_residue_census_minimal() {
    census(Runtime::without_stdlib(), "1");
}

/// Top-level closures in the USER program (no stdlib): if these leak beyond the
/// core-env floor, the defect is the top-level letrec shape itself, not the
/// compilation cache.
#[test]
#[ignore = "diagnostic: residue census for user top-level closures"]
fn teardown_residue_census_user_closures() {
    census(
        Runtime::without_stdlib(),
        "(def f (fn [x] (%add x 1))) (def g (fn [x] (f (f x)))) (g 1)",
    );
}

/// Minimal escaping-module shape: a top-level closure RETURNED as the program
/// value. The user_closures census (value = immediate) is clean; if this one
/// leaks, the defect is the program-value escape path.
#[test]
#[ignore = "diagnostic: residue census for escaping top-level closure"]
fn teardown_residue_census_escaping_closure() {
    census(Runtime::without_stdlib(), "(def f (fn [x] (%add x 1))) f");
}

/// A top-level binding captured by an escaping closure: the capture-cell chain
/// (cell + captured value + closure). With the program value released as a
/// process root, any residue above the floor is the capture-cell class.
#[test]
#[ignore = "diagnostic: residue census for captured top-level binding, escaping"]
fn teardown_residue_census_captured_escape() {
    census(
        Runtime::without_stdlib(),
        "(def a [1 2 3]) (def f (fn [] a)) f",
    );
}

/// A captured top-level CLOSURE binding (g, captured by f) reachable from the
/// escaping program value: the core.lisp letrec shape, minimized to two defs.
#[test]
#[ignore = "diagnostic: residue census for escaping mutual-capture closures"]
fn teardown_residue_census_mutual_capture_escape() {
    census(
        Runtime::without_stdlib(),
        "(def g (fn [x] x)) (def f (fn [x] (g x))) f",
    );
}

/// The module-export pattern: a struct literal of top-level closures as the
/// program value (core.lisp's final form, minimized).
#[test]
#[ignore = "diagnostic: residue census for struct-literal module export"]
fn teardown_residue_census_struct_export() {
    census(Runtime::without_stdlib(), "(def f (fn [x] x)) {:f f}");
}

/// An alias def (`(def reduce fold)` in core.lisp): two bindings, one value.
#[test]
#[ignore = "diagnostic: residue census for alias def"]
fn teardown_residue_census_alias_def() {
    census(
        Runtime::without_stdlib(),
        "(def f (fn [x] x)) (def h f) {:f f :h h}",
    );
}

/// core.lisp compiled as an ordinary USER program: if this stays at the 32-region
/// core-env floor, the user pipeline reclaims the very same program that
/// `compile_core` leaks — pinning the defect to compile_core's bespoke path.
#[test]
#[ignore = "diagnostic: residue census for core.lisp as user program"]
fn teardown_residue_census_core_as_user() {
    let src = std::fs::read_to_string("src/core.lisp").expect("core.lisp readable");
    census(Runtime::without_stdlib(), &src);
}

/// No user program at all: construct the runtime and tear it down. Any residue
/// is allocated by runtime setup (primitives, trait tables, core env), before
/// any compile.
#[test]
#[ignore = "diagnostic: residue census of bare runtime setup, no compile"]
fn teardown_residue_census_setup_only() {
    census_with(Runtime::without_stdlib(), None);
}
/// Diagnostic: does nested runtime list construction leak its interior regions
/// at top level (no macro involved)? If `(list 1 (list 2 3))` leaves residue
/// after its result is process-rooted and torn down, the leak is a GENERAL
/// RC-accounting bug in `list`/`append` lowering (interior call results not
/// released); if it is clean, the macro-expansion Pair leak is specific to the
/// expansion boundary (the transformer's whole output is discarded, not rooted).
///   `cargo test --test region_process_teardown teardown_residue_census_nested_list -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic: nested runtime list/append construction residue (no macro)"]
fn teardown_residue_census_nested_list() {
    eprintln!("--- (list 1 (list 2 3)) ---");
    census(Runtime::without_stdlib(), "(list 1 (list 2 3))");
    eprintln!("--- (list 1 (list 2 (list 3 4))) ---");
    census(Runtime::without_stdlib(), "(list 1 (list 2 (list 3 4)))");
}

/// Diagnostic: residue census for a concat loop — decomposes the dominant
/// runtime residue class by tag. `concat` is a core.lisp closure that folds
/// per-call lambdas over a mutable accumulator.
///   `cargo test --test region_process_teardown teardown_residue_census_concat_loop -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic: residue census for a concat loop (runtime leak class)"]
fn teardown_residue_census_concat_loop() {
    census(
        Runtime::new(),
        "(def @i 0) (while (%lt i 500) (concat \"x\" (number->string i)) (assign i (%add i 1)))",
    );
}

/// Diagnostic: residue census for a LOCAL self-referential `letrec` helper that
/// does not escape (the `(letrec [go (fn … (go …))] (go …))` idiom pervasive in
/// stdlib: `+`/`-`/`*`/`fold`/`push-all`/…). `go` is called and dies within the
/// activation, yet 2 regions/call leak: the Closure+ClosureTemplate region and
/// its self-reference CaptureCell. The <64-region edge dump reveals whether the
/// closure region and its capture-cell region form a cross-region cycle (each
/// holding a reference to the other), which RC alone cannot reclaim.
///   `cargo test --test region_process_teardown teardown_residue_census_inner_letrec -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic: residue census for a local self-referential letrec helper"]
fn teardown_residue_census_inner_letrec() {
    census(
        Runtime::without_stdlib(),
        "(def sumto (fn [x] (letrec [go (fn [acc xs] \
           (if (%eq xs 0) acc (go (%add acc xs) (%sub xs 1))))] (go 0 x)))) (sumto 3)",
    );
}

/// Diagnostic: residue census for a SINGLE prelude-macro expansion, no stdlib.
/// `when` expands via the quasiquote template `(if ,test (begin ,;body) nil)`,
/// which the transformer builds at runtime as a tree of `Pair`s (nested `list`
/// native calls). The residue here is small enough (<64 regions) that the
/// census prints the full heap graph + cross-ref edges — localizing exactly
/// which Pairs of the expansion output leak and how they are referenced.
///   `cargo test --test region_process_teardown teardown_residue_census_single_macro -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic: residue census for one macro expansion (Pair-leak localization)"]
fn teardown_residue_census_single_macro() {
    census(Runtime::without_stdlib(), "(when true 1)");
}

/// Compile but never execute: residue here is leaked by the compiler itself
/// (reader/expander/lowerer allocations whose regions never release), as
/// opposed to bytecode execution.
#[test]
#[ignore = "diagnostic: residue census of compile-only, no execute"]
fn teardown_residue_census_compile_only() {
    let mut rt = Runtime::without_stdlib();
    {
        let (_vm, symbols, cctx) = rt.parts();
        let _result = compile_file("1", symbols, cctx, "<census>").expect("compiles");
    }
    let report = rt.teardown();
    report_census(rt.heap(), &report);
}
