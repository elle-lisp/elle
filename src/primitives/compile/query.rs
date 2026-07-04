use std::collections::{BTreeMap, HashMap};

use crate::hir::symbols::extract_symbols_from_hir;
use crate::hir::{BindingArena, HirLinter};
use crate::hir::{Hir, HirKind};
use crate::pipeline::analyze_file;
use crate::signals::registry::with_registry;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::sorted_struct_get;
use crate::value::Value;

mod captures;
pub(crate) use captures::*;

use super::{
    build_binding_spans, build_call_graph, build_signal_map, call_edge_to_value,
    diagnostic_to_value, get_handle, kw, resolve_name, signal_to_value, symbol_def_to_value,
    AnalysisHandle,
};

/// `(compile/analyze source [opts])` → analysis handle
pub(super) fn prim_compile_analyze(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let source = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "compile/analyze: expected string source"),
            )
        }
    };

    // Optional opts struct for :file key.
    let file_name = if args.len() == 2 {
        if let Some(fields) = args[1].as_struct() {
            sorted_struct_get(fields, &kw("file"))
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_else(|| "<analyze>".to_string())
        } else {
            "<analyze>".to_string()
        }
    } else {
        "<analyze>".to_string()
    };

    // The driving VM hosts macro expansion during analysis. `ctx.vm()` is total
    // (a native always runs under a VM); read it as a raw pointer so it sits beside
    // the disjoint symbol-table and compile-context borrows below.
    let vm_ptr: *mut crate::vm::VM = ctx.vm();

    // The symbol table and compile context are disjoint siblings of the VM in the
    // owning `RuntimeCore`; read their pointers through the VM (this instance's
    // own) so all three become separate `&mut`.
    let symbols_ptr = unsafe { (*vm_ptr).symbols_ptr };
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error(
                "runtime-error",
                "compile/analyze: no symbol table in context",
            ),
        );
    }
    let cctx_ptr = unsafe { (*vm_ptr).compile_ctx_ptr };
    if cctx_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/analyze: no compile context"),
        );
    }
    let (symbols, vm, cctx) = unsafe { (&mut *symbols_ptr, &mut *vm_ptr, &mut *cctx_ptr) };

    // Run analysis.
    let result = match analyze_file(&source, symbols, vm, cctx, &file_name) {
        Ok(r) => r,
        Err(e) => return (SIG_ERROR, ctx.error("compile-error", e)),
    };

    // Extract symbols and diagnostics.
    let symbol_index = extract_symbols_from_hir(&result.hir, symbols, &result.arena);
    let mut linter = HirLinter::new();
    linter.lint(&result.hir, symbols, &result.arena);
    let mut diagnostics = linter.diagnostics().to_vec();

    // Convert accumulated analysis errors to diagnostics
    for err in &result.errors {
        use crate::error::ErrorKind;
        let (code, rule) = match &err.kind {
            ErrorKind::UndefinedVariable { .. } => ("E001", "undefined-variable"),
            ErrorKind::SignalMismatch { .. } => ("E002", "signal-mismatch"),
            ErrorKind::UnterminatedForm { .. } => ("E003", "unterminated-form"),
            ErrorKind::CompileError { .. } => ("E004", "compile-error"),
            _ => ("E000", "analysis-error"),
        };
        let loc = err
            .location
            .clone()
            .unwrap_or_else(|| crate::reader::SourceLoc::new(&file_name, 0, 0));
        diagnostics.push(crate::lint::diagnostics::Diagnostic::new(
            crate::lint::diagnostics::Severity::Error,
            code,
            rule,
            err.description(),
            Some(loc),
        ));
    }

    // Build signal map, call graph, and binding spans.
    let signal_map = build_signal_map(&result.hir, &result.arena, symbols);
    let call_graph = build_call_graph(&result.hir, &result.arena, symbols, &signal_map);

    let mut binding_spans = HashMap::new();
    build_binding_spans(
        &result.hir,
        &result.arena,
        symbols,
        &source,
        &symbol_index,
        &mut binding_spans,
    );

    let handle = AnalysisHandle {
        hir: result.hir,
        arena: result.arena,
        symbol_index,
        diagnostics,
        signal_map,
        call_graph,
        source: source.clone(),
        binding_spans,
    };

    (SIG_OK, ctx.external("analysis", handle))
}

/// (compile/diagnostics analysis) → [{:severity :warning :code "..." ...}]
pub(super) fn prim_compile_diagnostics(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/diagnostics", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let values: Vec<Value> = handle
        .diagnostics
        .iter()
        .map(|x| diagnostic_to_value(x, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}

/// (compile/symbols analysis) → [{:name "f" :kind :function ...}]
pub(super) fn prim_compile_symbols(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/symbols", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    // Only real in-file definitions (those with a source location). Usage-only
    // placeholder entries for primitives carry no location and are not symbols
    // the user defined here.
    let values: Vec<Value> = handle
        .symbol_index
        .definitions
        .values()
        .filter(|d| d.location.is_some())
        .map(|x| symbol_def_to_value(x, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}

/// (compile/signal analysis :name) → {:bits |:io| :propagates || ...}
pub(super) fn prim_compile_signal(
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
pub(super) fn prim_compile_query_signal(
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

/// (compile/bindings analysis) → [{:name "x" :scope :parameter ...}]
pub(super) fn prim_compile_bindings(
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

/// (compile/callers analysis :name) → [{:name "main" :line 50 :tail false}]
pub(super) fn prim_compile_callers(
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
pub(super) fn prim_compile_callees(
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
pub(super) fn prim_compile_call_graph(
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

/// (compile/binding analysis :name) → {:scope :local :mutated true ...}
pub(super) fn prim_compile_binding(
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
    let binding = match super::binding_for_name(&handle.arena, symbols, &handle.symbol_index, &name)
    {
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

// ── Primitive metadata ─────────────────────────────────────────────────

/// Return metadata for all Rust-defined primitives as an array of structs.
///
/// Each struct: {:name :category :arity :signal :doc :params :aliases}
pub(super) fn prim_compile_primitives(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let _ = args;
    use crate::primitives::registration::ALL_TABLES;

    let mut results = Vec::new();

    for table in ALL_TABLES {
        for def in *table {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), ctx.string(def.name));
            fields.insert(
                kw("category"),
                if def.category.is_empty() {
                    ctx.string("core")
                } else {
                    ctx.string(def.category)
                },
            );
            fields.insert(kw("arity"), ctx.string(format!("{}", def.arity)));
            fields.insert(kw("signal"), signal_to_value(&def.signal, ctx));
            fields.insert(kw("doc"), ctx.string(def.doc));

            let params: Vec<Value> = def.params.iter().map(|p| ctx.string(*p)).collect();
            fields.insert(kw("params"), ctx.array(params));

            let aliases: Vec<Value> = def.aliases.iter().map(|a| ctx.string(*a)).collect();
            fields.insert(kw("aliases"), ctx.array(aliases));

            results.push(ctx.struct_from(fields));
        }
    }

    (SIG_OK, ctx.array(results))
}
