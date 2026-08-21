//! Signal queries over an analysis handle: a single function's inferred signal,
//! and set-membership queries (`:io`, `:silent`, `:jit-eligible`, …).
use std::collections::BTreeMap;

use crate::primitives::compile::{get_handle, kw, resolve_name, signal_to_value};
use crate::signals::registry::with_registry;
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

/// (compile/query-signal analysis :io) → [{:name "f" :line 42}]
/// (compile/query-signal analysis :silent) → [{:name "g" :line 10}]
/// (compile/query-signal analysis :jit-eligible) → [...]
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

    let matches: Vec<Value> = with_registry(|reg| {
        handle
            .signal_map
            .iter()
            .filter(|(_, sig)| match query.as_str() {
                "silent" => sig.bits.is_empty() && sig.propagates == 0,
                "jit-eligible" => !sig.may_suspend(),
                "yields" => sig.may_suspend(),
                other => {
                    // Look up as a signal name.
                    if let Some(bit_pos) = reg.lookup(other) {
                        sig.bits.has_bit(bit_pos)
                    } else {
                        false
                    }
                }
            })
            .map(|(name, _)| {
                let mut fields = BTreeMap::new();
                fields.insert(kw("name"), ctx.string(&**name));
                // Find line from symbol index. Match only located definitions
                // so usage-only primitive placeholders (no location) never win.
                for def in handle.symbol_index.definitions.values() {
                    if def.name == *name {
                        if let Some(loc) = &def.location {
                            fields.insert(kw("line"), Value::int(loc.line as i64));
                            break;
                        }
                    }
                }
                ctx.struct_from(fields)
            })
            .collect()
    });

    (SIG_OK, ctx.array(matches))
}
