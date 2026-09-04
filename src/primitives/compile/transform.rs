use std::collections::{BTreeMap, BTreeSet};

use crate::hir::{Binding, HirKind};
use crate::rewrite::edit::{apply_edits, Edit};
use crate::signals::registry::with_registry;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::sorted_struct_get;
use crate::value::Value;

mod rewrite;
pub(crate) use rewrite::*;

use super::{
    collect_vars_in_range, compute_line_offsets, find_matching_paren, find_named_lambda,
    get_handle, kw, resolve_name, signal_to_value,
};

/// (compile/rename analysis :old-name :new-name) → {:source "..." :edits N}
pub(super) fn prim_compile_rename(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/rename", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let old_name = match resolve_name(args, 1, "compile/rename", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let new_name = match resolve_name(args, 2, "compile/rename", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/rename: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    // Pick the binding named old_name that carries source spans, skipping any
    // phantom file-scope prebind that shares the name but holds no spans.
    let target_binding =
        super::binding_for_name(&handle.arena, symbols, &handle.symbol_index, &old_name);
    let binding = match target_binding {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "lookup-error",
                    format!("compile/rename: no binding '{}' in analysis", old_name),
                ),
            )
        }
    };

    let name_spans = match handle.binding_spans.get(&binding) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (
                SIG_ERROR,
                ctx.error(
                    "lookup-error",
                    format!("compile/rename: no source spans for '{}'", old_name),
                ),
            )
        }
    };

    let mut edits: Vec<Edit> = name_spans
        .iter()
        .map(|(offset, len)| Edit {
            byte_offset: *offset,
            byte_len: *len,
            replacement: new_name.clone(),
        })
        .collect();

    let count = edits.len();
    match apply_edits(&handle.source, &mut edits) {
        Ok(new_source) => {
            let mut fields = BTreeMap::new();
            fields.insert(kw("source"), ctx.string(&*new_source));
            fields.insert(kw("edits"), Value::int(count as i64));
            (SIG_OK, ctx.struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            ctx.error("rewrite-error", format!("compile/rename: {}", e)),
        ),
    }
}

/// (compile/add-handler analysis :fn-name :signal-kind) → {:source "..." :wraps N}
pub(super) fn prim_compile_add_handler(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/add-handler", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let fn_name = match resolve_name(args, 1, "compile/add-handler", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let signal_kind = match resolve_name(args, 2, "compile/add-handler", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };

    // Verify the function emits the signal.
    let sig = match handle.signal_map.get(&fn_name) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "lookup-error",
                    format!("compile/add-handler: no function '{}'", fn_name),
                ),
            )
        }
    };

    let bit = match with_registry(|reg| reg.lookup(&signal_kind)) {
        Some(b) => b,
        None => match signal_kind.as_str() {
            "error" => 0,
            _ => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "lookup-error",
                        format!("compile/add-handler: unknown signal '{}'", signal_kind),
                    ),
                )
            }
        },
    };

    if !sig.bits.has_bit(bit) && sig.propagates & (1 << bit) == 0 {
        return (
            SIG_ERROR,
            ctx.error(
                "signal-error",
                format!(
                    "compile/add-handler: '{}' does not emit :{}",
                    fn_name, signal_kind
                ),
            ),
        );
    }

    // Find call sites via reverse call graph.
    let callers = handle
        .call_graph
        .reverse
        .get(&fn_name)
        .cloned()
        .unwrap_or_default();

    let line_offsets = compute_line_offsets(&handle.source);
    let mut edits = Vec::new();

    for caller_name in &callers {
        if let Some(edges) = handle.call_graph.edges.get(caller_name) {
            for edge in edges {
                if edge.callee == fn_name {
                    if let Some(&line_start) =
                        line_offsets.get((edge.line.saturating_sub(1)) as usize)
                    {
                        let byte_offset = line_start + (edge.col.saturating_sub(1)) as usize;
                        if let Some(call_end) = find_matching_paren(&handle.source, byte_offset) {
                            let call_text = &handle.source[byte_offset..call_end];
                            let wrapped = match signal_kind.as_str() {
                                "error" => format!(
                                    "(let [[ok? result] (protect {})] \
                                     (if ok? result (begin (eprintln \"error:\" result) nil)))",
                                    call_text
                                ),
                                "io" => format!("(with-timeout 5000 {})", call_text),
                                _ => format!("(protect {})", call_text),
                            };
                            edits.push(Edit {
                                byte_offset,
                                byte_len: call_end - byte_offset,
                                replacement: wrapped,
                            });
                        }
                    }
                }
            }
        }
    }

    let wrap_count = edits.len() as i64;
    match apply_edits(&handle.source, &mut edits) {
        Ok(new_source) => {
            let mut fields = BTreeMap::new();
            fields.insert(kw("source"), ctx.string(&*new_source));
            fields.insert(kw("wraps"), Value::int(wrap_count));
            (SIG_OK, ctx.struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            ctx.error("rewrite-error", format!("compile/add-handler: {}", e)),
        ),
    }
}

// ── compile/run-on ─────────────────────────────────────────────────────

/// `(compile/run-on tier f & args)` — force-dispatch `f` on the named tier.
///
/// Powers `lib/differential.lisp`. Returns the result, or signals
/// `:tier-rejected` if the tier doesn't accept this closure.
///
/// Tiers: `:bytecode`, `:jit`, `:mlir-cpu` (the last requires `--features mlir`).
///
/// Implementation: returns `SIG_QUERY` with payload `(tier closure arg1 arg2 ...)`;
/// the VM's `dispatch_compile_run_on` handler does the actual work because it
/// needs `&mut VM` access for the JIT cache, MLIR cache, and call machinery.
pub(super) fn prim_compile_run_on(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Cheap front-end validation — full type checks happen in the dispatch handler.
    if !args[0].is_keyword() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "compile/run-on: tier must be a keyword, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    if args[1].as_closure().is_none() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "compile/run-on: target must be a closure, got {}",
                    args[1].type_name()
                ),
            ),
        );
    }

    // Forward the entire arg list to the VM dispatcher.
    (
        SIG_QUERY,
        ctx.pair(Value::keyword("compile/run-on"), ctx.list(args.to_vec())),
    )
}

// ── compile/barrier-module ──────────────────────────────────────────────

/// `(compile/barrier-module source name)` — compile a file in the per-form
/// fault-barrier test mode (docs/test-runner.md § Mechanism). Returns a mutable
/// array of `[index thunk]` pairs (one 0-arg thunk per test/expression form,
/// each capturing the file's shared bindings), or signals an error on a
/// compile failure / def-initializer fault (a file-level failure for the runner).
///
/// Implementation: returns `SIG_QUERY` with payload `(source name)`; the VM's
/// `dispatch_barrier_module` handler does the work because it needs `&mut VM`
/// (symbol table from context, plus re-entrant module execution).
pub(super) fn prim_compile_barrier_module(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    for (i, label) in ["source", "name"].iter().enumerate() {
        if args[i].with_string(|_| ()).is_none() {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "compile/barrier-module: {} must be a string, got {}",
                        label,
                        args[i].type_name()
                    ),
                ),
            );
        }
    }
    (
        SIG_QUERY,
        ctx.pair(
            Value::keyword("compile/barrier-module"),
            ctx.list(args.to_vec()),
        ),
    )
}

/// `(compile/whole-module source name)` — compile a file as ONE whole-file thunk
/// (multi-form path): all top-level forms become the body of a single
/// 0-arg thunk, returned as one `[0 thunk]` entry. Unlike `compile/barrier-module`
/// (which hoists `def`/`var` eagerly and slices each expression into its own
/// thunk), this runs every form in source order, once per tier, in isolation —
/// matching a direct file run. See docs/test-runner.md § Multi-form files.
///
/// Like `compile/barrier-module`, returns `SIG_QUERY` (the VM handler does the
/// work — it needs the driving VM's own symbol table).
pub(super) fn prim_compile_whole_module(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    for (i, label) in ["source", "name"].iter().enumerate() {
        if args[i].with_string(|_| ()).is_none() {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "compile/whole-module: {} must be a string, got {}",
                        label,
                        args[i].type_name()
                    ),
                ),
            );
        }
    }
    (
        SIG_QUERY,
        ctx.pair(
            Value::keyword("compile/whole-module"),
            ctx.list(args.to_vec()),
        ),
    )
}

/// `(compile/read-forms source name)` — parse SOURCE into a list of syntax
/// values (spans preserved), without expanding or compiling. The companion to
/// `compile/whole-module-syntax`: the test runner reads a multi-form file
/// ONCE in the main VM with this, then ships the resulting syntax to a worker
/// (syntax is sendable across `os/spawn`) that compiles + runs it with its own
/// stdlib — so the file's runtime `import`s and the worker's `ev/run` scheduler
/// share one set of dynamic parameters. Reading needs no symbol table, so this
/// answers directly (no VM dispatch).
pub(super) fn prim_compile_read_forms(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let source = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "compile/read-forms: source must be a string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let name = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "compile/read-forms: name must be a string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    // Read into the call's region; each `ctx.syntax` wrapper then owns its
    // own copy of the form it wraps.
    let arena = ctx.syntax_arena();
    match crate::reader::read_syntax_all_for(arena, &source, &name) {
        Ok(forms) => {
            let vals: Vec<Value> = forms.into_iter().map(|s| ctx.syntax(s)).collect();
            (SIG_OK, ctx.list(vals))
        }
        Err(e) => (SIG_ERROR, ctx.error("compile-error", e)),
    }
}

/// `(compile/whole-module-syntax forms name)` — like `compile/whole-module`, but
/// from a list of already-parsed syntax values (from `compile/read-forms`)
/// instead of a source string. Returns one `[0 thunk]` entry. Returns
/// `SIG_QUERY` (the VM handler needs the driving VM's own symbol table) so the
/// WORKER that receives the shipped syntax compiles it against ITS OWN stdlib.
pub(super) fn prim_compile_whole_module_syntax(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[1].with_string(|_| ()).is_none() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "compile/whole-module-syntax: name must be a string, got {}",
                    args[1].type_name()
                ),
            ),
        );
    }
    (
        SIG_QUERY,
        ctx.pair(
            Value::keyword("compile/whole-module-syntax"),
            ctx.list(args.to_vec()),
        ),
    )
}

// ── compile/dumps ────────────────────────────────────────────────────────

/// `(compile/dumps source name)` — compile a module once through the real file
/// front-end and return a struct mapping each available dump kind to its
/// rendered text: `{:ast … :fhir … :defuse … :regions … :hir … :lir … :cfg …
/// :dfa … :jit … :escape …}`. These are the same artifacts `elle --dump=KIND`
/// prints, returned in-process instead of printed-and-exit, so the test runner
/// (`src/test.lisp`) can capture them per form into the CAS (docs/test-runner.md
/// § CAS asset capture). A stage that fails to compile or yields nothing is
/// omitted from the struct.
///
/// Implementation: returns `SIG_QUERY` with payload `(source name)`; the VM's
/// `dispatch_compile_dumps` handler does the work because it needs the driving
/// VM's own symbol table (same pattern as `compile/barrier-module`).
pub(super) fn prim_compile_dumps(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    for (i, label) in ["source", "name"].iter().enumerate() {
        if args[i].with_string(|_| ()).is_none() {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "compile/dumps: {} must be a string, got {}",
                        label,
                        args[i].type_name()
                    ),
                ),
            );
        }
    }
    (
        SIG_QUERY,
        ctx.pair(Value::keyword("compile/dumps"), ctx.list(args.to_vec())),
    )
}
