// audited: 2026-10-06
//! A tail `or`/`and` that returns a short-circuit operand hands the caller an owning reference to it.
//!
//! docs/impl/region/mechanism.md

use super::*;

/// A tail-position `(or param …)` (or `(and …)`) that SHORT-CIRCUIT-returns an owned heap
/// param must hand the caller an owning reference to it, exactly like `(if param param …)`.
/// The return-wrapping pass (src/hir/return_incref.rs) wraps the whole `or`/`and` in
/// `Return` for that reason.
///
/// Counterfactual: with `Return` pushed into the last operand only, the short-circuit value
/// gets no mint, its owned-param decref frees it before the return, and the caller reads a
/// freed string (`tag/object mismatch`). `mq` is not self-recursive, so the file pins the
/// short-circuit return mint alone.
#[test]
fn tail_or_short_circuit_returns_owned_param_no_uaf() {
    use crate::pipeline::compile_file_repl;
    let src = "(def m ((fn [] \
        (defn mq [url] (or url \"z\")) \
        {:run (fn [] (mq \"hi\"))}))) \
        (m:run)";
    let mut rt = Runtime::new();
    let res = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, _symbols, cctx) = rt.parts();
    let v = vm
        .execute_scheduled(&res, cctx)
        .expect("a tail `(or param …)` returning an owned heap param must not double-free it");
    assert!(
        v.is_string(),
        "the passthrough `(or url \"z\")` returns the string param \"hi\", got {v:?}"
    );
}
