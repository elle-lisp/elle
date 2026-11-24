// audited: 2026-11-21
//! `compile/lir`: a unit's frozen LIR as Elle data, which the VM answers because it holds the compiler.
//!
//! docs/spec/form.md

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::types::Arity;
use crate::value::Value;

primitive! {
    "compile/lir" => prim_compile_lir {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Compile SOURCE (NAME for error spans) through the file front end and return each function of the unit as a struct in the data form (docs/spec/form.md): the entry function, then each closure in the order of its closure id. Each block lists its instructions, each instruction names its :kind and its fields, and :slots names the binding each local slot holds. Signals :compile-error on a source that does not compile.",
        params: &["source", "name"],
        category: "compile",
        example: r#"(compile/lir "(defn f [x] (+ x 1))" "<eval>")"#,
        // Both arguments are copied out to `&str`, and the answer is built
        // fresh, so nothing is stored. The query dispatch mints the answer
        // rather than this call's own region, so the result is unbounded:
        // `Opaque`, as `compile/dumps` is (docs/impl/region/effects.md).
        effect: RegionEffect::Opaque,
    }
}

/// `(compile/lir source name)` — returns `SIG_QUERY`, and the VM's
/// `dispatch_compile_lir` compiles with its own symbol table.
fn prim_compile_lir(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    super::transform::source_query(ctx, args, "compile/lir")
}
