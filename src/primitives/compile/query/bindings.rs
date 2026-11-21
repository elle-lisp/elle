// audited: 2026-11-20
//! `compile/bindings` and `compile/binding`: each binding the source introduces, or one named binding with its usages.
//!
//! docs/analysis/portrait.md

use std::collections::{BTreeMap, HashSet};

use crate::hir::Binding;
use crate::primitives::compile::{binding_for_name, get_handle, kw, resolve_name, AnalysisHandle};
use crate::primitives::ctx::NativeCtx;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::Value;

/// The fields both queries answer for `binding`, named `name`: its scope, its
/// mutation and capture flags, and where the source defines it.
fn binding_fields(
    handle: &AnalysisHandle,
    binding: Binding,
    name: &str,
    ctx: &mut NativeCtx,
) -> BTreeMap<TableKey, Value> {
    let inner = handle.arena.get(binding);
    let mut fields = BTreeMap::new();
    fields.insert(kw("name"), ctx.string(name));
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
    fields
}

/// (compile/bindings analysis) → [{:name "x" :scope :parameter ...}]
pub(in crate::primitives::compile) fn prim_compile_bindings(
    ctx: &mut NativeCtx<'_>,
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

    // The tree, not the arena: the arena also holds every primitive, and a
    // binding the analysis made and the tree never introduces. A macro that
    // places a body more than once binds its names once per copy, at one
    // place, so one place answers once.
    let mut introduced = Vec::new();
    handle.hir.introduced_bindings(&mut introduced);
    let mut seen = HashSet::new();
    let mut values = Vec::new();
    for binding in introduced {
        let inner = handle.arena.get(binding);
        if inner.is_primitive || inner.is_synthetic || inner.is_macro_introduced {
            continue;
        }
        let Some(name) = symbols.name(inner.name) else {
            continue;
        };
        let at = handle
            .symbol_index
            .symbol_locations
            .get(&binding.def_id())
            .map(|loc| (loc.line, loc.col));
        if !seen.insert((name, at)) {
            continue;
        }
        let fields = binding_fields(handle, binding, name, ctx);
        values.push(ctx.struct_from(fields));
    }
    (SIG_OK, ctx.array(values))
}

/// (compile/binding analysis :name) → {:scope :local :mutated true ...}
pub(in crate::primitives::compile) fn prim_compile_binding(
    ctx: &mut NativeCtx<'_>,
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

    let Some(binding) = binding_for_name(&handle.arena, symbols, &handle.symbol_index, &name)
    else {
        return (
            SIG_ERROR,
            ctx.error(
                "lookup-error",
                format!("compile/binding: no binding '{}' in analysis", name),
            ),
        );
    };
    let mut fields = binding_fields(handle, binding, &name, ctx);

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
