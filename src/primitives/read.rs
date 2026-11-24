// audited: 2026-11-24
//! The read primitives, `read` and `read-all`: source text in, the data each form writes out.
//!
//! docs/syntax.md
//!
//! `tests/lang/reader.lisp` and `tests/lang/datum.lisp` pin them.

use crate::primitives::def::RegionEffect;
use crate::reader::{read_syntax, read_syntax_all};
use crate::signals::Signal;
use crate::syntax::Syntax;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// The data of each form in `forms`, in the ctx's region, or the error that
/// names `prim`. The table is reached through the driving VM, the instance's
/// own; a raw deref, because `to_value` takes both the table and `ctx`.
fn data_of(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    forms: &[Syntax],
    prim: &str,
) -> Result<Vec<Value>, (SignalBits, Value)> {
    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        let msg = format!("{}: symbol table not available", prim);
        return Err((SIG_ERROR, ctx.error("internal-error", msg)));
    }
    let symbols = unsafe { &mut *symbols_ptr };
    let mut values = Vec::with_capacity(forms.len());
    for form in forms {
        match form.to_value(symbols, ctx) {
            Ok(value) => values.push(value),
            Err(e) => {
                let msg = format!("{}: {}", prim, e);
                return Err((SIG_ERROR, ctx.error("type-error", msg)));
            }
        }
    }
    Ok(values)
}

/// `(read str)`: the data of the first form in `str`.
pub(crate) fn prim_read(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(source) = args[0].with_string(|s| s.to_string()) else {
        return type_error!(ctx, args[0], "read", "string");
    };
    // The tree is born in the call's region and lives exactly as long as the
    // call, because only its data leaves.
    let arena = ctx.syntax_arena();
    let syntax = match read_syntax(arena, &source, "<read>") {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("read-error", e)),
    };
    match data_of(ctx, &[syntax], "read") {
        Ok(values) => (SIG_OK, values[0]),
        Err(err) => err,
    }
}

/// `(read-all str)`: the data of every form in `str`, as a list.
pub(crate) fn prim_read_all(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(source) = args[0].with_string(|s| s.to_string()) else {
        return type_error!(ctx, args[0], "read-all", "string");
    };
    let arena = ctx.syntax_arena();
    let syntaxes = match read_syntax_all(arena, &source, "<read>") {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("read-error", e)),
    };
    // Each form and the list that holds them share the call's region.
    match data_of(ctx, &syntaxes, "read-all") {
        Ok(values) => (SIG_OK, ctx.list(values)),
        Err(err) => err,
    }
}

primitive! {
    "read" => prim_read {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse the first form from a string and answer its data. A literal is the value it \
              writes, a struct or set literal included; a bytes literal is a call of bytes. A \
              struct key that no struct holds signals :type-error.",
        params: &["str"],
        category: "meta",
        example: "(read \"(+ 1 2)\") #=> (+ 1 2)",
        effect: RegionEffect::Fresh,
    }
    "read-all" => prim_read_all {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse every form from a string and answer a list of their data, each as read \
              answers it.",
        params: &["str"],
        category: "meta",
        example: "(read-all \"1 2 3\") #=> (1 2 3)",
        effect: RegionEffect::Fresh,
    }
}
