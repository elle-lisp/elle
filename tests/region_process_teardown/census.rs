use super::*;

/// The sweep must be *observable* and *idempotent*. The residual live-region
/// count is the standing oracle: it is the set of regions whose RC never reached
/// zero — the open leaks (the leak-suite canaries, tests/elle/leak*.lisp) — and
/// falls to zero as those are fixed, with no change to the teardown itself.
#[test]
fn process_teardown_is_observable_and_idempotent() {
    let mut rt = Runtime::new();

    // A representative program — define and use bindings, like any user file.
    let src = "(def squares (map (fn [x] (* x x)) (list 1 2 3)))";
    {
        let (vm, symbols, cctx) = rt.parts();
        let result = compile_file(src, symbols, cctx, "<teardown-test>").expect("compiles");
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
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
/// runtime leak class (the leak*.lisp residue) by tag. `concat` is a core.lisp
/// closure that folds per-call lambdas over a mutable accumulator.
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
