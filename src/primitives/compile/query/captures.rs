// audited: 2026-11-20
//! `compile/captures` and `compile/captured-by`: what a named function captures, and who captures a name.
//!
//! docs/analysis/portrait.md

use super::*;
use crate::hir::{BindingInner, CaptureInfo};
use crate::primitives::compile::{defined_name, each_definition, find_named_lambda};
use crate::primitives::ctx::NativeCtx;

/// Whether a capture of `inner` is one the source sees. A template's binding is
/// not the source's, and a compiler temporary is nobody's. Every function in a
/// file shares the file's immutable bindings, so a reference to one captures
/// nothing; a `var` of the file is a capture, because a closure can write it.
fn reported(inner: &BindingInner) -> bool {
    !inner.is_macro_introduced
        && !inner.is_synthetic
        && !(inner.is_file_scope && inner.is_immutable)
}

/// (compile/captures analysis :fn-name) → [{:name "x" :kind :value :mutated false}]
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

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captures: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    let lambda = find_named_lambda(&handle.hir, &handle.arena, symbols, &name)
        .and_then(|value| value.kind.as_lambda());
    match lambda {
        Some(lambda) => {
            let captures = captures_to_values(&lambda.captures, &handle.arena, symbols, ctx);
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

/// Each capture the source sees, as `{:name :kind :mutated}`.
pub(crate) fn captures_to_values(
    captures: &[CaptureInfo],
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    ctx: &mut NativeCtx,
) -> Vec<Value> {
    captures
        .iter()
        .filter(|cap| reported(arena.get(cap.binding)))
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
                crate::hir::CaptureKind::Local | crate::hir::CaptureKind::Recursive { .. } => {
                    if inner.needs_capture() {
                        "lbox"
                    } else {
                        "value"
                    }
                }
                crate::hir::CaptureKind::Capture { .. } => "transitive",
            };
            fields.insert(kw("kind"), Value::keyword(kind));
            fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
            ctx.struct_from(fields)
        })
        .collect()
}

/// (compile/captured-by analysis :name) → [{:name "make-handler" :line 20}]
/// Reverse lookup: which functions capture the named binding?
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

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captured-by: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };
    let arena = &handle.arena;

    // Each function the source defines whose captures include one named `name`.
    let mut capturers = Vec::new();
    each_definition(&handle.hir, &mut |binding, value| {
        let (Some(fn_name), Some(lambda)) = (
            defined_name(binding, value, arena, symbols),
            value.kind.as_lambda(),
        ) else {
            return;
        };
        let captures_it = lambda.captures.iter().any(|cap| {
            let inner = arena.get(cap.binding);
            reported(inner) && symbols.name(inner.name) == Some(name.as_str())
        });
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
