//! Primitives that query a `compile/analyze` handle. The handlers are grouped
//! by concern into submodules; this root re-exports them so the registration
//! table in the parent `compile` module still resolves each `query::prim_*`.
use std::collections::BTreeMap;

use crate::hir::{BindingArena, Hir, HirKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

// `kw`/`signal_to_value` serve `prim_compile_primitives` below; the remaining
// names (`get_handle`, `resolve_name`) are re-exported into scope for the
// `captures` submodule, which pulls them via `use super::*`.
use super::{get_handle, kw, resolve_name, signal_to_value};

mod analysis;
mod bindings;
mod captures;
mod graph;
mod signals;

pub(crate) use captures::*;

pub(super) use analysis::{prim_compile_analyze, prim_compile_diagnostics, prim_compile_symbols};
pub(super) use bindings::{prim_compile_binding, prim_compile_bindings};
pub(super) use graph::{prim_compile_call_graph, prim_compile_callees, prim_compile_callers};
pub(super) use signals::{prim_compile_query_signal, prim_compile_signal};

// ── Primitive metadata ─────────────────────────────────────────────────

/// Return metadata for all Rust-defined primitives as an array of structs.
///
/// Each struct: {:name :category :arity :signal :doc :params :aliases}
pub(super) fn prim_compile_primitives(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let _ = args;
    use crate::primitives::registration::ALL_TABLES;

    let mut results = Vec::new();

    for table in ALL_TABLES {
        for def in *table {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(def.name));
            fields.insert(
                kw("category"),
                if def.category.is_empty() {
                    ctx.string("core")
                } else {
                    ctx.string(def.category)
                },
            );
            fields.insert(kw("arity"), ctx.string(format!("{}", def.arity)));
            fields.insert(kw("signal"), signal_to_value(&def.signal, ctx));
            fields.insert(kw("doc"), ctx.string(def.doc));

            let params: Vec<Value> = def.params.iter().map(|p| ctx.string(*p)).collect();
            fields.insert(kw("params"), ctx.array(params));

            let aliases: Vec<Value> = def.aliases.iter().map(|a| ctx.string(*a)).collect();
            fields.insert(kw("aliases"), ctx.array(aliases));

            results.push(ctx.struct_from(fields));
        }
    }

    (SIG_OK, ctx.array(results))
}
