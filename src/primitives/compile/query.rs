// audited: 2026-11-21
//! The primitives that query a `compile/analyze` handle, grouped by concern into submodules, and `compile/primitives`.
//!
//! docs/analysis/portrait.md
//!
//! This root re-exports the handlers, so the registration table in the parent
//! `compile` module resolves each `query::prim_*`.
use std::collections::BTreeMap;

use crate::hir::BindingArena;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

// `kw`/`signal_to_value` serve `prim_compile_primitives` below; the remaining
// names (`get_handle`, `resolve_name`) are re-exported into scope for the
// `captures` submodule, which pulls them via `use super::*`.
use super::{get_handle, kw, resolve_name, signal_to_value};
use crate::primitives::ctx::NativeCtx;
use crate::primitives::spec::{Kind, Mode, Place, Spec};

mod analysis;
mod bindings;
mod captures;
mod exports;
mod graph;
mod signals;

pub(crate) use captures::*;

pub(super) use analysis::{prim_compile_analyze, prim_compile_diagnostics, prim_compile_symbols};
pub(super) use bindings::{prim_compile_binding, prim_compile_bindings};
pub(super) use exports::prim_compile_exports;
pub(super) use graph::{prim_compile_call_graph, prim_compile_callees, prim_compile_callers};
pub(super) use signals::{prim_compile_query_signal, prim_compile_signal};

// ── Primitive metadata ─────────────────────────────────────────────────

/// Return metadata for all Rust-defined primitives as an array of structs.
///
/// Each struct: {:name :category :arity :signal :doc :params :aliases :fn
/// :effect :effect-args :embeds :moves-out :spec}
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

            // The native's function, which a `ValueConst` loads, and the
            // region declaration this implementation reads for it
            // (docs/spec/form.md). A variant's name reaches the keyword
            // through `effect_of`, so the instance's memo learns it.
            let (effect, effect_args) = effect_of(def.effect);
            let ints = |ctx: &crate::primitives::ctx::NativeCtx<'_>, ns: &[usize]| {
                ctx.array(ns.iter().map(|&n| Value::int(n as i64)).collect())
            };
            fields.insert(kw("fn"), Value::native_fn(def));
            fields.insert(kw("effect"), ctx.keyword(effect));
            fields.insert(kw("effect-args"), ints(ctx, effect_args));
            fields.insert(kw("embeds"), ints(ctx, def.embeds));
            fields.insert(kw("moves-out"), Value::bool(def.moves_out));
            // The native's declared signature, or nil (docs/spec/signature.md).
            let spec = def.spec.map_or(Value::NIL, |spec| spec_of(spec, ctx));
            fields.insert(kw("spec"), spec);

            results.push(ctx.struct_from(fields));
        }
    }

    (SIG_OK, ctx.array(results))
}

/// A region declaration's variant in kebab case, and the arguments it names.
fn effect_of(effect: crate::primitives::def::RegionEffect) -> (&'static str, &'static [usize]) {
    use crate::primitives::def::RegionEffect as E;
    match effect {
        E::Immediate => ("immediate", &[]),
        E::Fresh => ("fresh", &[]),
        E::PassThrough => ("pass-through", &[]),
        E::Stores { args } => ("stores", args),
        E::Sends { args } => ("sends", args),
        E::Funnel => ("funnel", &[]),
        E::Opaque => ("opaque", &[]),
        E::Delivers { args } => ("delivers", args),
        E::Mixed => ("mixed", &[]),
        E::Unknown => ("unknown", &[]),
    }
}

/// A native's declared signature as the data form writes it:
/// `{:params :rest :result}`.
fn spec_of(spec: Spec, ctx: &mut NativeCtx<'_>) -> Value {
    let params: Vec<Value> = spec
        .params()
        .iter()
        .map(|&mode| mode_of(mode, ctx))
        .collect();
    let mut fields = BTreeMap::new();
    fields.insert(kw("params"), ctx.array(params));
    let rest = spec
        .rest_mode()
        .map_or(Value::NIL, |mode| mode_of(mode, ctx));
    fields.insert(kw("rest"), rest);
    let result = match spec.result() {
        Kind::Immediate => ctx.keyword("immediate"),
        Kind::Fresh => ctx.keyword("fresh"),
        Kind::Alias => ctx.keyword("alias"),
        Kind::Itself(i) => tagged("itself", Value::int(i as i64), ctx),
        Kind::Payload(i) => tagged("payload", Value::int(i as i64), ctx),
    };
    fields.insert(kw("result"), result);
    ctx.struct_from(fields)
}

/// A mode as a keyword, or a store as `[:stored place]`.
fn mode_of(mode: Mode, ctx: &mut NativeCtx<'_>) -> Value {
    match mode {
        Mode::Borrowed => ctx.keyword("borrowed"),
        Mode::Returned => ctx.keyword("returned"),
        Mode::Crosses => ctx.keyword("crosses"),
        Mode::Resumed => ctx.keyword("resumed"),
        Mode::Stored(Place::Result) => {
            let place = ctx.keyword("result");
            tagged("stored", place, ctx)
        }
        Mode::Stored(Place::Param(j)) => {
            let place = tagged("param", Value::int(j as i64), ctx);
            tagged("stored", place, ctx)
        }
    }
}

/// `[:tag value]`, the form a mode or a kind takes when it names something.
fn tagged(tag: &str, value: Value, ctx: &mut NativeCtx<'_>) -> Value {
    let tag = ctx.keyword(tag);
    ctx.array(vec![tag, value])
}
