use super::*;

/// Per-compile region GROWTH census — the corpus-OOM instrument. The runner
/// keeps ONE long-lived VM across hundreds of files (`elle test FILE...`), so
/// any region a compile allocates and never releases accumulates without bound
/// and eventually SIGKILLs `make smoke` at ~45GB. A flat
/// compiler reclaims each compile's scratch and this stays at zero growth; a
/// per-compile leak shows linear growth in some tag class.
///
/// Compile the SAME source N times in one runtime (no teardown, no execute),
/// and report the live-region histogram delta per tag class, normalized to
/// per-compile. Run with:
///   `cargo test --test region_process_teardown per_compile_region_growth -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic: per-compile region growth, bucketed by tag class"]
fn per_compile_region_growth() {
    // A ladder of source shapes, simplest first, to localize WHICH construct
    // leaks per compile (reader-only literal → macro call → fn/closure → let).
    let cases: &[(&str, &str)] = &[
        // Non-macro shapes are flat — the per-compile leak is entirely macro
        // expansion.
        ("int literal      ", "1"),
        ("primitive call   ", "(%add 1 1)"),
        ("string literal   ", "\"hello\""),
        ("list literal     ", "(list 1 2 3)"),
        ("fn closure       ", "(fn [x] (%add x 1))"),
        ("let binding      ", "(let [x 1] (%add x 1))"),
        ("def + use        ", "(def y 5) (%add y 1)"),
        // Prelude-macro expansions. Closure+ClosureTemplate growth is now 0 (the
        // transformer is compiled once and shared, not re-compiled per compile);
        // residual growth is the per-expansion primitive-allocation leak below.
        ("assert macro     ", "(assert (= (+ 1 1) 2) \"ok\")"),
        ("when macro       ", "(when (= 1 1) 2)"),
        ("defn macro       ", "(defn f [x] (%add x 1))"),
        // defmacro discriminators: a FILE-LOCAL macro's transformer is not shared
        // with the master, so its compiled closure still leaks +1/compile (a
        // separate, smaller class — only files that define AND use a macro).
        // It is per-transformer-compile, not per-call: 1, 2, 3 calls all = +1.
        ("defmacro+1call   ", "(defmacro m [x] x) (m 5)"),
        ("defmacro+3calls  ", "(defmacro m [x] x) (m 1) (m 2) (m 3)"),
        // The residual per-EXPANSION leak (scales with calls, not compiles):
        // `string` called inside a transformer body leaks its LString result —
        // a primitive routing its allocation outside the expansion's transient
        // region.
        (
            "xform string x1  ",
            "(defmacro m [x] (let [_ (string \"a\" \"b\")] `(%add ,x 1))) (m 5)",
        ),
        (
            "xform string x2  ",
            "(defmacro m [x] (let [_ (string \"a\" \"b\")] `(%add ,x 1))) (m 5) (m 6)",
        ),
    ];
    for (label, src) in cases {
        per_compile_growth_one(label, src);
    }
}

/// Minimal repro for the reclaim over-free: build a runtime (stdlib load runs
/// thousands of macro expansions through the scope reclaim) then recompile,
/// which reads the cached stdlib closures — a stale read here means reclaim
/// freed a region still reachable from a cache.
#[test]
fn macro_scope_reclaim_does_not_overfree_caches() {
    // Two full Runtime lifecycles on one thread: each instance builds its own
    // trait-method tables on its own heap, so the second runtime's stdlib load
    // re-exercises trait dispatch from inside macro expansion. A stale read here
    // means the scope reclaim freed a region the trait registry still holds.
    for _ in 0..3 {
        let mut rt = Runtime::new();
        {
            let (_vm, symbols, cctx) = rt.parts();
            let _ =
                compile_file("(when true (+ 1 1))", symbols, cctx, "<repro>").expect("compiles");
        }
        let _ = rt.teardown();
    }
}
/// Regression gate for the per-compile macro-TRANSFORMER leak (the dominant
/// share of the corpus-OOM growth). Each prelude macro's transformer must be
/// compiled ONCE and shared across the per-compile `Expander` clones via an
/// `Rc<RefCell<…>>` cell on the persistent compilation-cache master, so repeated
/// compiles add NO `Closure`/`ClosureTemplate` regions. Recompiling the
/// transformer into a fresh region per compile would orphan it when the clone
/// drops (`Value` is `Copy`, no decref), accumulating regions on the runner's
/// one long-lived VM until `make smoke` is SIGKILLed (it reached ~45GB).
///
/// The gate is the transformer class specifically (`Closure`/`ClosureTemplate`):
/// `assert`'s expansion still grows other classes per call (`LString` from the
/// `string` calls in its body), so a total-growth==0 gate would conflate the two.
/// This one pins exactly what the cache fix guarantees.
///
/// Invariant pinned: repeated compiles of `(assert …)` add zero closure
/// regions (a per-compile transformer recompile would add ≈2 each).
#[test]
fn macro_transformer_is_not_recompiled_per_compile() {
    let mut rt = Runtime::new();
    let src = "(assert (= (+ 1 1) 2) \"ok\")";
    let n = 20;
    let closure_growth = {
        let (vm, symbols, cctx) = rt.parts();
        // Warm-up compile absorbs the one-time first transformer compile; after
        // it, a shared transformer adds no further closure regions.
        let _ = compile_file(src, symbols, cctx, "<leak>").expect("compiles");
        let base = *region_class_histogram(vm.heap())
            .get("Closure+ClosureTemplate")
            .unwrap_or(&0) as i64;
        for _ in 0..n {
            let _ = compile_file(src, symbols, cctx, "<leak>").expect("compiles");
        }
        *region_class_histogram(vm.heap())
            .get("Closure+ClosureTemplate")
            .unwrap_or(&0) as i64
            - base
    };
    let _ = rt.teardown();
    assert_eq!(
        closure_growth, 0,
        "prelude-macro transformer re-compiled per compile: {closure_growth} \
         closure regions leaked over {n} compiles — the cache cell is no longer \
         shared across Expander clones"
    );
}

/// Counterfactual for the macro-expansion **Pair** leak — the dominant
/// teardown-residue class (~10.8k Pair regions after a stdlib load; the largest
/// remaining leak class).
///
/// SPEC: macro expansion is a COMPILE-TIME activity. A transformer builds its
/// quasiquote output as a transient tree of runtime `Value`s — nested `list` /
/// `append` / `array` native calls (see `quasiquote_to_code`). `from_value`
/// then deep-copies that tree into owned `Syntax`, after which every `Value`
/// the transformer allocated is dead scratch: the constructed output, the
/// `append`-discarded segment lists, all of it. A flat compiler reclaims that
/// scratch, so compiling the SAME macro-using source repeatedly must not grow
/// the live `Pair`-region population. This is the same flat-compiler contract
/// that `macro_transformer_is_not_recompiled_per_compile` pins for the
/// transformer closure, here for its construction output.
///
/// Invariant pinned: repeated compiles of `(when …)` add zero Pair regions
/// once the transformer's whole allocation scratch is reclaimed. `(when …)`
/// lowers to `(list 'if test (append (list 'begin) body) nil)`; if each
/// intermediate `list`/`append` result kept an unbalanced Rule-5 escape incref
/// that no `decref_point` in the transformer body released (macro_expand.rs
/// releases only the single root region), ~4 Pair regions would leak per
/// compile.
#[test]
fn macro_expansion_output_pairs_are_reclaimed() {
    let mut rt = Runtime::new();
    let src = "(when true 1)";
    let n = 20;
    let pair_growth = {
        let (vm, symbols, cctx) = rt.parts();
        // Warm-up compile absorbs the one-time transformer compile; after it, a
        // flat compiler adds no further Pair regions per compile.
        let _ = compile_file(src, symbols, cctx, "<leak>").expect("compiles");
        let base = *region_class_histogram(vm.heap()).get("Pair").unwrap_or(&0) as i64;
        for _ in 0..n {
            let _ = compile_file(src, symbols, cctx, "<leak>").expect("compiles");
        }
        *region_class_histogram(vm.heap()).get("Pair").unwrap_or(&0) as i64 - base
    };
    let _ = rt.teardown();
    assert_eq!(
        pair_growth, 0,
        "macro expansion leaked {pair_growth} Pair regions over {n} compiles of \
         `{src}`: the transformer's quasiquote-construction intermediates \
         (list/append results) retain an unbalanced escape incref and are never \
         reclaimed — the dominant teardown-residue class"
    );
}

/// The end-state target, pinned but not yet reachable: zero regions survive a
/// full run + teardown. RED until the leak-suite canaries are fixed; the
/// teardown scaffolding does not change when it greens — only the leaks do.
/// Kept as an `#[ignore]`'d standing oracle so `cargo test -- --ignored` reports
/// the current residue as the remaining-work number.
#[test]
#[ignore = "RED until the leak-suite canaries (tests/elle/leak*.lisp) are fixed; reports current residue"]
fn process_teardown_frees_all_regions() {
    let mut rt = Runtime::new();
    {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file("(+ 1 2)", symbols, cctx, "<teardown-target>").expect("compiles");
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
    }
    let report = rt.teardown();
    assert_eq!(
        report.live_regions, 0,
        "regions still live after teardown (residue = open leaks): {:?}",
        report.regions
    );
}
