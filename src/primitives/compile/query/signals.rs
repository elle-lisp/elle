// audited: 2026-11-20
//! `compile/signal` and `compile/query-signal`: one function's inferred signal, and the functions a signal query matches.
//!
//! docs/analysis/portrait.md

use std::collections::BTreeMap;

use crate::primitives::compile::{get_handle, kw, resolve_name, signal_to_value};
use crate::signals::registry::with_registry;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

/// (compile/signal analysis :name) → {:bits |:io| :propagates || ...}
pub(in crate::primitives::compile) fn prim_compile_signal(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/signal", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/signal", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match handle.signal_map.get(&name) {
        Some(sig) => (SIG_OK, signal_to_value(sig, ctx)),
        None => (
            SIG_ERROR,
            ctx.error(
                "lookup-error",
                format!("compile/signal: no function '{}' in analysis", name),
            ),
        ),
    }
}

/// The test a signal query names: `:silent`, `:yields`, or a registered
/// signal's keyword, by its bit.
enum SignalQuery {
    Silent,
    Yields,
    Bit(u32),
}

impl SignalQuery {
    /// The query `query` names, or `None` when it names nothing.
    fn parse(query: &str) -> Option<Self> {
        match query {
            "silent" => Some(SignalQuery::Silent),
            "yields" => Some(SignalQuery::Yields),
            other => with_registry(|reg| reg.lookup(other)).map(SignalQuery::Bit),
        }
    }

    fn matches(&self, sig: &Signal) -> bool {
        match self {
            SignalQuery::Silent => sig.is_silent(),
            SignalQuery::Yields => sig.may_park(),
            SignalQuery::Bit(bit) => sig.bits.has_bit(*bit),
        }
    }
}

/// (compile/query-signal analysis :io) → [{:name "f" :line 42}]
/// (compile/query-signal analysis :silent) → [{:name "g" :line 10}]
pub(in crate::primitives::compile) fn prim_compile_query_signal(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/query-signal", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let query = match resolve_name(args, 1, "compile/query-signal", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let Some(signal_query) = SignalQuery::parse(&query) else {
        return (
            SIG_ERROR,
            ctx.error(
                "lookup-error",
                format!("compile/query-signal: '{query}' is neither :silent, :yields nor a signal"),
            ),
        );
    };

    let mut names: Vec<&String> = handle
        .signal_map
        .iter()
        .filter(|(_, sig)| signal_query.matches(sig))
        .map(|(name, _)| name)
        .collect();
    names.sort();
    let values: Vec<Value> = names
        .into_iter()
        .map(|name| {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(&**name));
            // The line of the definition. Only a located definition answers,
            // so a usage-only primitive placeholder never wins.
            let line = handle
                .symbol_index
                .definitions
                .values()
                .filter(|def| def.name == *name)
                .find_map(|def| def.location.as_ref());
            if let Some(loc) = line {
                fields.insert(kw("line"), Value::int(loc.line as i64));
            }
            ctx.struct_from(fields)
        })
        .collect();
    (SIG_OK, ctx.array(values))
}
