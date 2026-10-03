// audited: 2026-09-29
//! Per-compile region growth: what compiling the same source again in one runtime may add.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/diagnostics.md

use super::*;

/// Per-compile region GROWTH census. A VM that compiles many sources in one
/// process keeps every region a compile allocates and never releases, so a
/// per-compile leak grows without bound there. A flat compiler reclaims each
/// compile's scratch and reads zero growth; a per-compile leak shows linear
/// growth in some tag class.
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
        // Non-macro shapes: every one reads zero.
        ("int literal      ", "1"),
        ("primitive call   ", "(%add 1 1)"),
        ("string literal   ", "\"hello\""),
        ("list literal     ", "(list 1 2 3)"),
        ("fn closure       ", "(fn [x] (+ x 1))"),
        ("let binding      ", "(let [x 1] (+ x 1))"),
        ("def + use        ", "(def y 5) (+ y 1)"),
        // Prelude-macro expansions read zero: the transformer is compiled once
        // and shared across compiles.
        ("assert macro     ", "(assert (= (+ 1 1) 2) \"ok\")"),
        ("when macro       ", "(when (= 1 1) 2)"),
        ("defn macro       ", "(defn f [x] (+ x 1))"),
        // A FILE-LOCAL macro's transformer is not shared with the master, so
        // its compiled closure adds one Closure+ClosureTemplate region per
        // compile. It is per transformer compile, not per call: one call and
        // three calls both read +1.
        ("defmacro+1call   ", "(defmacro m [x] x) (m 5)"),
        ("defmacro+3calls  ", "(defmacro m [x] x) (m 1) (m 2) (m 3)"),
        // A primitive that allocates inside a transformer body (`string`
        // here): its result is transformer scratch, so one expansion and two
        // read the same, the +1 of the file-local transformer alone.
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

/// A runtime's stdlib load runs thousands of macro expansions through the scope
/// reclaim, and a recompile then reads the cached stdlib closures. A stale read
/// here means the reclaim freed a region a cache still reaches.
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

/// Each prelude macro's transformer must be compiled ONCE and shared across the
/// per-compile `Expander` clones via an `Rc<RefCell<…>>` cell on the persistent
/// compilation-cache master, so repeated compiles add NO
/// `Closure`/`ClosureTemplate` regions. Recompiling the transformer into a fresh
/// region per compile would orphan it when the clone drops (`Value` is `Copy`,
/// no decref), accumulating regions in any VM that compiles many sources.
///
/// The gate reads the transformer class alone (`Closure`/`ClosureTemplate`), so
/// growth in another class does not move it; `per_compile_region_growth` reads
/// every class.
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

/// Macro expansion is a COMPILE-TIME activity. A transformer builds its
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
/// Invariant pinned: repeated compiles of `(when …)` add zero Pair regions once
/// the transformer's whole allocation scratch is reclaimed. The counter-factual:
/// `(when …)` lowers to `(list 'if test (append (list 'begin) body) nil)`; if
/// each intermediate `list`/`append` result kept an unbalanced Rule-5 escape
/// incref that no `decref_point` in the transformer body released
/// (src/syntax/expand/macro_expand.rs releases only the single root region), ~4
/// Pair regions would leak per compile.
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
         reclaimed"
    );
}
