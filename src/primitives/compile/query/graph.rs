//! Call-graph queries over an analysis handle: callers of a function, callees
//! of a function, and the whole graph as nodes/roots/leaves.
use std::collections::BTreeMap;

use crate::primitives::compile::{call_edge_to_value, get_handle, kw, resolve_name};
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// (compile/callers analysis :name) → [{:name "main" :line 50 :tail false}]
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

    let callers = handle
        .call_graph
        .reverse
        .get(&name)
        .cloned()
        .unwrap_or_default();

    let values: Vec<Value> = callers
        .iter()
        .map(|caller_name| {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(&**caller_name));
            // Find the specific edge for line info.
            if let Some(edges) = handle.call_graph.edges.get(caller_name) {
                for edge in edges {
                    if edge.callee == name {
                        fields.insert(kw("line"), Value::int(edge.line as i64));
                        fields.insert(kw("tail"), Value::bool(edge.is_tail));
                        break;
                    }
                }
            }
            ctx.struct_from(fields)
        })
        .collect();

    (SIG_OK, ctx.array(values))
}

/// (compile/callees analysis :name) → [{:name "http/get" :line 3 :tail false}]
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

    let edges = handle
        .call_graph
        .edges
        .get(&name)
        .cloned()
        .unwrap_or_default();

    let values: Vec<Value> = edges.iter().map(|x| call_edge_to_value(x, ctx)).collect();
    (SIG_OK, ctx.array(values))
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

    let nodes: Vec<Value> = handle
        .call_graph
        .edges
        .iter()
        .map(|(name, edges)| {
            let mut fields = BTreeMap::new();
            let name_val = ctx.string(&**name);
            fields.insert(kw("name"), name_val);
            let callees: Vec<Value> = edges.iter().map(|e| ctx.string(&*e.callee)).collect();
            let callees_val = ctx.array(callees);
            fields.insert(kw("callees"), callees_val);
            let callers = handle
                .call_graph
                .reverse
                .get(name)
                .cloned()
                .unwrap_or_default();
            let caller_vals: Vec<Value> = callers.iter().map(|c| ctx.string(&**c)).collect();
            let callers_val = ctx.array(caller_vals);
            fields.insert(kw("callers"), callers_val);
            ctx.struct_from(fields)
        })
        .collect();

    let mut fields = BTreeMap::new();
    let nodes_val = ctx.array(nodes);
    fields.insert(kw("nodes"), nodes_val);
    let roots: Vec<Value> = handle
        .call_graph
        .roots
        .iter()
        .map(|s| ctx.string(&**s))
        .collect();
    let roots_val = ctx.array(roots);
    fields.insert(kw("roots"), roots_val);
    let leaves: Vec<Value> = handle
        .call_graph
        .leaves
        .iter()
        .map(|s| ctx.string(&**s))
        .collect();
    let leaves_val = ctx.array(leaves);
    fields.insert(kw("leaves"), leaves_val);

    (SIG_OK, ctx.struct_from(fields))
}
