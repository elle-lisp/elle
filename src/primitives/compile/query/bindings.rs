//! Binding queries over an analysis handle: enumerate all bindings, or inspect
//! a single named binding (scope, mutation, capture, source spans, usages).
use std::collections::BTreeMap;

use crate::primitives::compile::{binding_for_name, get_handle, kw, resolve_name};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

/// (compile/bindings analysis) → [{:name "x" :scope :parameter ...}]
pub(in crate::primitives::compile) fn prim_compile_bindings(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/bindings", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/bindings: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    let mut values = Vec::new();
    for i in 0..handle.arena.len() {
        let binding = crate::hir::Binding(i as u32);
        let inner = handle.arena.get(binding);
        let mut fields = BTreeMap::new();
        if let Some(name) = symbols.name(inner.name) {
            fields.insert(kw("name"), ctx.string(name));
        } else {
            continue; // Skip gensym bindings.
        }
        fields.insert(
            kw("scope"),
            Value::keyword(match inner.scope {
                crate::hir::arena::BindingScope::Parameter => "parameter",
                crate::hir::arena::BindingScope::Local => "local",
            }),
        );
        fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
        fields.insert(kw("immutable"), Value::bool(inner.is_immutable));
        fields.insert(kw("needs-lbox"), Value::bool(inner.needs_capture()));

        // Add location from symbol index if available (keyed per-binding).
        if let Some(loc) = handle.symbol_index.symbol_locations.get(&binding.def_id()) {
            fields.insert(kw("line"), Value::int(loc.line as i64));
            fields.insert(kw("col"), Value::int(loc.col as i64));
        }

        values.push(ctx.struct_from(fields));
    }
    (SIG_OK, ctx.array(values))
}

/// (compile/binding analysis :name) → {:scope :local :mutated true ...}
pub(in crate::primitives::compile) fn prim_compile_binding(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/binding", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/binding", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/binding: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    // Find the binding by name, preferring the one that carries source spans
    // (skips any phantom file-scope prebind sharing the name).
    let binding = match binding_for_name(&handle.arena, symbols, &handle.symbol_index, &name) {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "lookup-error",
                    format!("compile/binding: no binding '{}' in analysis", name),
                ),
            )
        }
    };
    let inner = handle.arena.get(binding);
    let mut fields = BTreeMap::new();
    fields.insert(
        kw("name"),
        ctx.string(symbols.name(inner.name).unwrap_or("")),
    );
    fields.insert(
        kw("scope"),
        Value::keyword(match inner.scope {
            crate::hir::arena::BindingScope::Parameter => "parameter",
            crate::hir::arena::BindingScope::Local => "local",
        }),
    );
    fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
    fields.insert(kw("immutable"), Value::bool(inner.is_immutable));
    fields.insert(kw("needs-lbox"), Value::bool(inner.needs_capture()));

    if let Some(loc) = handle.symbol_index.symbol_locations.get(&binding.def_id()) {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }

    // Usages (keyed per-binding).
    if let Some(usages) = handle.symbol_index.symbol_usages.get(&binding.def_id()) {
        let usage_vals: Vec<Value> = usages
            .iter()
            .map(|loc| {
                let mut f = BTreeMap::new();
                f.insert(kw("line"), Value::int(loc.line as i64));
                f.insert(kw("col"), Value::int(loc.col as i64));
                ctx.struct_from(f)
            })
            .collect();
        fields.insert(kw("usages"), ctx.array(usage_vals));
    }

    (SIG_OK, ctx.struct_from(fields))
}
