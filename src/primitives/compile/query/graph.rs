// audited: 2026-11-20
//! `compile/callers`, `compile/callees` and `compile/call-graph`: the calls the source spells, read off an analysis handle.
//!
//! docs/analysis/portrait.md

use std::collections::BTreeMap;

use crate::primitives::compile::{call_to_value, get_handle, kw, resolve_name};
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// (compile/callers analysis :name) → [{:name "main" :line 50 :col 3 :tail false}]
pub(in crate::primitives::compile) fn prim_compile_callers(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/callers", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/callers", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let sites = handle.call_graph.reverse.get(&name);
    let values: Vec<Value> = sites
        .into_iter()
        .flatten()
        .map(|site| call_to_value(&site.caller, site.at, None, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}

/// (compile/callees analysis :name) → [{:name "http/get" :line 3 :col 5 :tail false :signal {…}}]
pub(in crate::primitives::compile) fn prim_compile_callees(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/callees", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/callees", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let edges = handle.call_graph.edges.get(&name);
    let values: Vec<Value> = edges
        .into_iter()
        .flatten()
        .map(|edge| call_to_value(&edge.callee, edge.at, edge.signal.as_ref(), ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}

/// Each name once, in the order it first appears.
fn unique<'a>(
    names: impl Iterator<Item = &'a str>,
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
) -> Value {
    let mut seen = std::collections::HashSet::new();
    let values: Vec<Value> = names
        .filter(|name| seen.insert(*name))
        .map(|name| ctx.string(name))
        .collect();
    ctx.array(values)
}

/// (compile/call-graph analysis) → {:nodes [...] :roots [...] :leaves [...]}
pub(in crate::primitives::compile) fn prim_compile_call_graph(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/call-graph", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let graph = &handle.call_graph;

    let nodes: Vec<Value> = graph
        .edges
        .iter()
        .map(|(name, edges)| {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(&**name));
            let callees = unique(edges.iter().map(|e| e.callee.as_str()), ctx);
            fields.insert(kw("callees"), callees);
            let sites = graph.reverse.get(name).into_iter().flatten();
            let callers = unique(sites.map(|s| s.caller.as_str()), ctx);
            fields.insert(kw("callers"), callers);
            ctx.struct_from(fields)
        })
        .collect();

    let mut fields = BTreeMap::new();
    fields.insert(kw("nodes"), ctx.array(nodes));
    let roots = unique(graph.roots.iter().map(String::as_str), ctx);
    fields.insert(kw("roots"), roots);
    let leaves = unique(graph.leaves.iter().map(String::as_str), ctx);
    fields.insert(kw("leaves"), leaves);
    (SIG_OK, ctx.struct_from(fields))
}
