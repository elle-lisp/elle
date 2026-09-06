// audited: 2026-09-06
// docs/impl/wasm.md
//! The uncaught-error oracle: which programs must fail the run, and which
//! must not.
//!
//! An error that reaches the top of the program must make `eval_wasm` return
//! `Err`, the way the VM exits nonzero. Returning the raised value as a normal
//! `Ok` result made this tier a weak oracle: every uncaught error was a silent
//! exit-0 false-pass that hid real failures — `assert`s included — under
//! `--wasm=full`.

use super::*;

#[test]
fn wasm_full_uncaught_error_fails() {
    // A bare `(error …)`, a failed `assert`, and `ev/run`'s re-raise of an
    // unjoined errored fiber all reach the top of the program, so each must
    // report `Err` (the CLI then exits nonzero).
    for src in [
        "(error {:e 9})",
        "(assert false \"boom\")",
        "(ev/join (ev/spawn (fn [] (error {:e 1}))))",
        "(defer (fn [] nil) (ev/join (ev/spawn (fn [] (error {:e 3})))))",
    ] {
        let r = super::super::eval_wasm_with_stdlib(src, "<uncaught>");
        assert!(
            r.is_err(),
            "uncaught top-level error must fail under --wasm=full, got Ok for {src:?}: {r:?}"
        );
    }
}

#[test]
fn wasm_full_uncaught_error_report_spells_the_names_it_carries() {
    // The report an uncaught error prints is all the author gets, and a keyword
    // in the raised value is a name hash: only the instance memo can spell it
    // (docs/impl/symbol.md § "Reading a name, and not reading one"). The trap:
    // the keywords the Rust runtime mints from fixed strings — `:error`,
    // `:message` — resolve through the static vocabulary with no memo at all, so
    // a test written with those names goes green while every name the program
    // itself coined prints as `#<keyword:hash>`. The counter-factual: assert
    // only that the report is an `Err`, and a report of raw hashes passes for
    // the error the author named.
    let report = super::super::eval_wasm_with_stdlib("(error {:sigil-wasm-kind 9})", "<uncaught>")
        .expect_err("an uncaught error must fail under --wasm=full");
    assert!(
        report.contains(":sigil-wasm-kind"),
        "the report must spell the keyword the raised value carries, got: {report}"
    );
    assert!(
        !report.contains("#<keyword:"),
        "no keyword in the report may fall back to its hash, got: {report}"
    );
}

#[test]
fn wasm_full_gated_skip_is_not_a_failure() {
    // A `(gate! …)` whose condition is unmet raises a `:gated` error that the
    // harness records as SKIP (an unbuilt plugin/feature), NOT a failure. The
    // uncaught-error oracle must exit cleanly for it — matching the VM driver's
    // `take_gated_exit_reason` — so gated corpus files stay exit-0 under
    // `--wasm=full` instead of being counted as errors.
    let r = super::super::eval_wasm_with_stdlib("(gate! false \"feature not built\" 1)", "<gated>");
    assert!(
        r.is_ok(),
        "a gated skip must not fail under --wasm=full, got {r:?}"
    );
}

#[test]
fn wasm_full_caught_or_valued_error_does_not_fail() {
    // The oracle must fire only on a RAISED error: a caught error (`protect`), an
    // error-shaped VALUE returned without raising, and a caught-then-continue
    // program all complete normally (`Ok`). Guards the uncaught-error oracle
    // against flagging clean runs whose result merely looks error-shaped.
    for (src, expect) in [
        ("(protect (error {:e 1}))", "[false {:e 1}]"),
        ("{:error :not-raised}", "{:error :not-raised}"),
        ("(do (protect (error {:e 1})) 42)", "42"),
    ] {
        assert_eq!(
            eval_with_stdlib(src),
            expect,
            "a non-raising program must complete normally under --wasm=full: {src:?}"
        );
    }
}
