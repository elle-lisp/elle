// audited: 2026-11-21
//! `compile/captures` and `compile/captured-by`: the slots a compiled closure holds, and the closures that hold a name.
//!
//! docs/analysis/portrait.md

use super::*;
use crate::hir::{BindingInner, CaptureInfo, CaptureKind};
use crate::primitives::compile::{defined_name, each_definition, find_named_lambda};
use crate::primitives::ctx::NativeCtx;

/// Where a slot came from: `:compiler` for a slot a pass added or a compiler
/// temporary, `:macro` for a template's binding, and `:source` for the rest.
fn origin(cap: &CaptureInfo, inner: &BindingInner) -> Value {
    if cap.is_added() || inner.is_synthetic {
        Value::keyword("compiler")
    } else if inner.is_macro_introduced {
        Value::keyword("macro")
    } else {
        Value::keyword("source")
    }
}

/// (compile/captures analysis :fn-name) → [{:name "x" :kind :value :mutated false :origin :source}]
pub(crate) fn prim_compile_captures(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captures", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captures", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let compiled = match handle.compiled("compile/captures", ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captures: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    let lambda = find_named_lambda(&compiled.hir, &compiled.arena, symbols, &name)
        .and_then(|value| value.kind.as_lambda());
    match lambda {
        Some(lambda) => {
            let captures = captures_to_values(&lambda.captures, &compiled.arena, symbols, ctx);
            (SIG_OK, ctx.array(captures))
        }
        None => (
            SIG_ERROR,
            ctx.error(
                "lookup-error",
                format!("compile/captures: no function '{}' in analysis", name),
            ),
        ),
    }
}

/// Each slot, in slot order, as `{:name :kind :mutated :origin}`. A slot
/// whose binding has no spelling, such as a hoisted constant's, has no
/// `:name`.
fn captures_to_values(
    captures: &[CaptureInfo],
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    ctx: &mut NativeCtx,
) -> Vec<Value> {
    captures
        .iter()
        .map(|cap| {
            let inner = arena.get(cap.binding);
            let mut fields = BTreeMap::new();
            if let Some(name) = symbols.name(inner.name) {
                let name_val = ctx.string(name);
                fields.insert(kw("name"), name_val);
            }
            let kind = match cap.kind {
                // A `Recursive` self-edge is reported like a `Local` capture: `lbox`
                // when the binding keeps a cell, `value` when it is cell-free.
                CaptureKind::Local | CaptureKind::Recursive { .. } => {
                    if inner.needs_capture() {
                        "lbox"
                    } else {
                        "value"
                    }
                }
                CaptureKind::Capture { .. } => "transitive",
            };
            fields.insert(kw("kind"), Value::keyword(kind));
            fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
            fields.insert(kw("origin"), origin(cap, inner));
            ctx.struct_from(fields)
        })
        .collect()
}

/// (compile/captured-by analysis :name) → [{:name "make-handler" :line 20}]
/// Reverse lookup: which functions' closures hold a binding of that name?
pub(crate) fn prim_compile_captured_by(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captured-by", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captured-by", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let compiled = match handle.compiled("compile/captured-by", ctx) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captured-by: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };
    let arena = &compiled.arena;

    // Each function the source defines whose closure holds one named `name`.
    let mut capturers = Vec::new();
    each_definition(&compiled.hir, &mut |binding, value| {
        let (Some(fn_name), Some(lambda)) = (
            defined_name(binding, value, arena, symbols),
            value.kind.as_lambda(),
        ) else {
            return;
        };
        let captures_it = lambda
            .captures
            .iter()
            .any(|cap| symbols.name(arena.get(cap.binding).name) == Some(name.as_str()));
        if captures_it {
            capturers.push((fn_name, value.span.line));
        }
    });
    let results: Vec<Value> = capturers
        .into_iter()
        .map(|(fn_name, line)| {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(fn_name));
            fields.insert(kw("line"), Value::int(line as i64));
            ctx.struct_from(fields)
        })
        .collect();
    (SIG_OK, ctx.array(results))
}
