// audited: 2026-09-06
// docs/impl/wasm.md
//! What compile-time macro expansion needs from the full-module path.
//!
//! The full-module WASM path (`eval_wasm_with_stdlib`) splices stdlib SOURCE
//! into the compiled unit so stdlib functions become WASM at runtime. But macro
//! expansion still runs at COMPILE time on the context's macro VM, and a prelude
//! macro's transformer body may call a stdlib function: `assert`'s transformer
//! (src/prelude.lisp) calls `pair?` (a stdlib `defn`, src/stdlib.lisp) to decide
//! whether the asserted form is a comparison. If the compile context's macro VM
//! never loaded stdlib, that call resolves to nothing and expansion dies with
//! "undefined variable: pair?" — which took down nearly every corpus file under
//! `--wasm=full`, since virtually all of them use `assert`. These pin that the
//! full-module path loads stdlib into the macro-expansion environment.

use super::*;

#[test]
fn wasm_full_expands_assert_macro() {
    // `(assert true)` is the minimal trigger: `assert`'s transformer calls the
    // stdlib `pair?` at expansion time. Before the macro VM loaded stdlib this
    // failed to compile at all. The asserted form is truthy, so it yields `true`.
    assert_eq!(
        eval_with_stdlib("(assert true)"),
        "true",
        "assert's transformer calls the stdlib `pair?`; the full-module WASM \
         path must load stdlib into the macro-expansion environment"
    );
}

#[test]
fn wasm_full_expands_macro_calling_stdlib_function() {
    // The defect generalizes past `assert`: ANY user macro whose transformer
    // body calls a stdlib function must expand. This one branches on `pair?` of
    // its literal argument — a compile-time stdlib call independent of prelude.
    assert_eq!(
        eval_with_stdlib("(defmacro m [x] (if (pair? x) `1 `2))\n(m (a b))"),
        "1",
        "a user macro calling the stdlib `pair?` at expansion time must resolve \
         it under the full-module WASM path"
    );
}

#[test]
fn wasm_full_bakes_quoted_symbol_literal() {
    // A compound quoted literal with a *symbol* leaf (`(= a 2)` is a list of the
    // symbols `=`, `a` and the int `2`) reaches the emitter as a
    // `MaterializeConst` of a `ConstTemplate::Pair(...Symbol...)`. Baking it into
    // the const pool interns each symbol into the driving instance's table; with
    // no table threaded, `materialize` panicked ("no symbol table for a quoted
    // symbol"). Pins the WasmEmitter::symbols wiring. Reducing to a bool with
    // `=` proves the baked symbol interned to the SAME id the reader gives a
    // fresh `=` — and keeps the returned value immediate (see `eval`'s caveat).
    assert_eq!(
        eval_with_stdlib("(= (first (quote (= a 2))) (quote =))"),
        "true",
        "a quoted compound literal's symbol leaves must bake into the const pool \
         and intern to the reader's ids under the full-module WASM path"
    );
}

#[test]
fn wasm_full_expands_comparison_assert() {
    // The realistic corpus shape: `(assert (= L R))` takes `assert`'s comparison
    // branch, which embeds `(quote (= 1 1))` — a compound SYMBOL literal — into
    // the expansion. It exercises BOTH defects at once: the transformer calls the
    // stdlib `pair?` (macro-expansion resolution) AND the expansion bakes a
    // quoted-symbol literal (const-pool interning). Nearly every corpus file uses
    // this form, so it is what took the `--wasm=full` pass down. Truthy → `true`.
    assert_eq!(
        eval_with_stdlib("(assert (= 1 1) \"one equals one\")"),
        "true",
        "a comparison `assert` must both expand (stdlib `pair?`) and bake its \
         quoted-form literal under the full-module WASM path"
    );
}
