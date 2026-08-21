//! Read primitives (string → value)
use crate::primitives::def::RegionEffect;
use crate::reader::{read_syntax, read_syntax_all};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Parse the first form from a string.
///
/// `(read str)` → parsed value
///
/// ```lisp
/// (read "(+ 1 2)")   # → '(+ 1 2)
/// (read "42")         # → 42
/// (read "true")       # → true
/// ```
pub(crate) fn prim_read(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let source = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "read", "string");
    };

    // Parse the first form
    let syntax = match read_syntax(&source, "<read>") {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("read-error", e)),
    };

    // Convert Syntax to Value — needs the symbol table for interning symbols.
    // Reached through the driving VM (this instance's own table). Raw deref:
    // `to_value` takes both the table and `ctx`.
    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("internal-error", "read: symbol table not available"),
        );
    }
    let symbols = unsafe { &mut *symbols_ptr };

    (SIG_OK, syntax.to_value(symbols, ctx))
}

/// Parse all forms from a string.
///
/// `(read-all str)` → list of parsed values
///
/// ```lisp
/// (read-all "1 2 3")  # → (1 2 3)
/// (read-all "(+ 1 2) (- 3 4)")  # → ((+ 1 2) (- 3 4))
/// ```
pub(crate) fn prim_read_all(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let source = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "read-all", "string");
    };

    let syntaxes = match read_syntax_all(&source, "<read>") {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, ctx.error("read-error", e)),
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("internal-error", "read-all: symbol table not available"),
        );
    }
    let symbols = unsafe { &mut *symbols_ptr };

    // Each form materializes through the ctx (its region), as does the list
    // result that wraps them — one region for the whole reply.
    let mut values: Vec<Value> = Vec::with_capacity(syntaxes.len());
    for s in &syntaxes {
        values.push(s.to_value(symbols, ctx));
    }
    (SIG_OK, ctx.list(values))
}

primitive! {
    "read" => prim_read {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse the first form from a string, returning a value",
        params: &["str"],
        category: "meta",
        example: "(read \"(+ 1 2)\") #=> (+ 1 2)",
        effect: RegionEffect::Fresh,
    }
    "read-all" => prim_read_all {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse all forms from a string, returning a list of values",
        params: &["str"],
        category: "meta",
        example: "(read-all \"1 2 3\") #=> (1 2 3)",
        effect: RegionEffect::Fresh,
    }
}
