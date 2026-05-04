use std::collections::{BTreeMap, HashMap};

use crate::context;
use crate::hir::symbols::extract_symbols_from_hir;
use crate::hir::{BindingArena, HirLinter};
use crate::hir::{Hir, HirKind};
use crate::pipeline::analyze_file;
use crate::signals::registry::with_registry;
use crate::value::error_val;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::sorted_struct_get;
use crate::value::Value;

use super::{
    build_binding_spans, build_call_graph, build_signal_map, call_edge_to_value,
    diagnostic_to_value, get_handle, kw, resolve_name, signal_to_value, symbol_def_to_value,
    AnalysisHandle,
};

/// `(compile/analyze source [opts])` → analysis handle
pub(super) fn prim_compile_analyze(args: &[Value]) -> (SignalBits, Value) {
    let source = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "compile/analyze: expected string source"),
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

    // We need mutable access to the symbol table and a VM for macro
    // expansion.  Use the thread-local context.
    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "runtime-error",
                    "compile/analyze: no symbol table in context",
                ),
            )
        }
    };
    let vm_ptr = match context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/analyze: no VM in context"),
            )
        }
    };

    let (symbols, vm) = unsafe { (&mut *symbols_ptr, &mut *vm_ptr) };

    // Run analysis.
    let result = match analyze_file(&source, symbols, vm, &file_name) {
        Ok(r) => r,
        Err(e) => return (SIG_ERROR, error_val("compile-error", e)),
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

    (SIG_OK, Value::external("analysis", handle))
}

/// (compile/diagnostics analysis) → [{:severity :warning :code "..." ...}]
pub(super) fn prim_compile_diagnostics(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/diagnostics") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let values: Vec<Value> = handle.diagnostics.iter().map(diagnostic_to_value).collect();
    (SIG_OK, Value::array(values))
}

/// (compile/symbols analysis) → [{:name "f" :kind :function ...}]
pub(super) fn prim_compile_symbols(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/symbols") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let values: Vec<Value> = handle
        .symbol_index
        .definitions
        .values()
        .map(symbol_def_to_value)
        .collect();
    (SIG_OK, Value::array(values))
}

/// (compile/signal analysis :name) → {:bits |:io| :propagates || ...}
pub(super) fn prim_compile_signal(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/signal") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/signal") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match handle.signal_map.get(&name) {
        Some(sig) => (SIG_OK, signal_to_value(sig)),
        None => (
            SIG_ERROR,
            error_val(
                "lookup-error",
                format!("compile/signal: no function '{}' in analysis", name),
            ),
        ),
    }
}

/// (compile/query-signal analysis :io) → [{:name "f" :line 42}]
/// (compile/query-signal analysis :silent) → [{:name "g" :line 10}]
/// (compile/query-signal analysis :jit-eligible) → [...]
pub(super) fn prim_compile_query_signal(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/query-signal") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let query = match resolve_name(args, 1, "compile/query-signal") {
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
                fields.insert(kw("name"), Value::string(&**name));
                // Find line from symbol index.
                for def in handle.symbol_index.definitions.values() {
                    if def.name == *name {
                        if let Some(loc) = &def.location {
                            fields.insert(kw("line"), Value::int(loc.line as i64));
                        }
                        break;
                    }
                }
                Value::struct_from(fields)
            })
            .collect()
    });

    (SIG_OK, Value::array(matches))
}

/// (compile/bindings analysis) → [{:name "x" :scope :parameter ...}]
pub(super) fn prim_compile_bindings(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/bindings") {
        Ok(h) => h,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/bindings: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    let mut values = Vec::new();
    for i in 0..handle.arena.len() {
        let binding = crate::hir::Binding(i as u32);
        let inner = handle.arena.get(binding);
        let mut fields = BTreeMap::new();
        if let Some(name) = symbols.name(inner.name) {
            fields.insert(kw("name"), Value::string(name));
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
        fields.insert(kw("captured"), Value::bool(inner.is_captured));
        fields.insert(kw("immutable"), Value::bool(inner.is_immutable));
        fields.insert(kw("needs-lbox"), Value::bool(inner.needs_capture()));

        // Add location from symbol index if available.
        if let Some(loc) = handle.symbol_index.symbol_locations.get(&inner.name) {
            fields.insert(kw("line"), Value::int(loc.line as i64));
            fields.insert(kw("col"), Value::int(loc.col as i64));
        }

        values.push(Value::struct_from(fields));
    }
    (SIG_OK, Value::array(values))
}

/// (compile/captures analysis :fn-name) → [{:name "x" :kind :value :mutated false}]
pub(super) fn prim_compile_captures(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captures") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captures") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/captures: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Find the Lambda for this function name.
    match find_lambda_captures(&handle.hir, &handle.arena, symbols, &name) {
        Some(captures) => (SIG_OK, Value::array(captures)),
        None => (
            SIG_ERROR,
            error_val(
                "lookup-error",
                format!("compile/captures: no function '{}' in analysis", name),
            ),
        ),
    }
}

pub(super) fn find_lambda_captures(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
) -> Option<Vec<Value>> {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let Some(name) = symbols.name(arena.get(*binding).name) {
                    if name == target {
                        if let HirKind::Lambda { captures, .. } = &value.kind {
                            return Some(captures_to_values(captures, arena, symbols));
                        }
                    }
                }
                if let Some(result) = find_lambda_captures(value, arena, symbols, target) {
                    return Some(result);
                }
            }
            find_lambda_captures(body, arena, symbols, target)
        }
        HirKind::Define { binding, value } => {
            if let Some(name) = symbols.name(arena.get(*binding).name) {
                if name == target {
                    if let HirKind::Lambda { captures, .. } = &value.kind {
                        return Some(captures_to_values(captures, arena, symbols));
                    }
                }
            }
            find_lambda_captures(value, arena, symbols, target)
        }
        HirKind::Lambda { body, .. } => find_lambda_captures(body, arena, symbols, target),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_lambda_captures(cond, arena, symbols, target)
            .or_else(|| find_lambda_captures(then_branch, arena, symbols, target))
            .or_else(|| find_lambda_captures(else_branch, arena, symbols, target)),
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                if let Some(r) = find_lambda_captures(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Block { body, .. } => {
            for e in body {
                if let Some(r) = find_lambda_captures(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Call { func, args, .. } => find_lambda_captures(func, arena, symbols, target)
            .or_else(|| {
                for arg in args {
                    if let Some(r) = find_lambda_captures(&arg.expr, arena, symbols, target) {
                        return Some(r);
                    }
                }
                None
            }),
        HirKind::Assign { value, .. } => find_lambda_captures(value, arena, symbols, target),
        HirKind::While { cond, body } => find_lambda_captures(cond, arena, symbols, target)
            .or_else(|| find_lambda_captures(body, arena, symbols, target)),
        HirKind::Match { value, arms } => {
            if let Some(r) = find_lambda_captures(value, arena, symbols, target) {
                return Some(r);
            }
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    if let Some(r) = find_lambda_captures(g, arena, symbols, target) {
                        return Some(r);
                    }
                }
                if let Some(r) = find_lambda_captures(body, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            find_lambda_captures(e, arena, symbols, target)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                if let Some(r) = find_lambda_captures(c, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_lambda_captures(b, arena, symbols, target) {
                    return Some(r);
                }
            }
            else_branch
                .as_ref()
                .and_then(|e| find_lambda_captures(e, arena, symbols, target))
        }
        HirKind::Destructure { value, .. } => find_lambda_captures(value, arena, symbols, target),
        HirKind::Eval { expr, env } => find_lambda_captures(expr, arena, symbols, target)
            .or_else(|| find_lambda_captures(env, arena, symbols, target)),
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                if let Some(r) = find_lambda_captures(p, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_lambda_captures(v, arena, symbols, target) {
                    return Some(r);
                }
            }
            find_lambda_captures(body, arena, symbols, target)
        }
        _ => None,
    }
}

pub(super) fn captures_to_values(
    captures: &[crate::hir::CaptureInfo],
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> Vec<Value> {
    captures
        .iter()
        .map(|cap| {
            let inner = arena.get(cap.binding);
            let mut fields = BTreeMap::new();
            if let Some(name) = symbols.name(inner.name) {
                fields.insert(kw("name"), Value::string(name));
            }
            let kind = match cap.kind {
                crate::hir::CaptureKind::Local => {
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
            Value::struct_from(fields)
        })
        .collect()
}

/// (compile/captured-by analysis :name) → [{:name "make-handler" :line 20}]
/// Reverse lookup: which functions capture the named binding?
pub(super) fn prim_compile_captured_by(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captured-by") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captured-by") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/captured-by: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Find all lambdas whose captures include a binding named `name`.
    let mut results = Vec::new();
    find_capturers(&handle.hir, &handle.arena, symbols, &name, &mut results);
    (SIG_OK, Value::array(results))
}

pub(super) fn find_capturers(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
    results: &mut Vec<Value>,
) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let HirKind::Lambda { captures, .. } = &value.kind {
                    for cap in captures {
                        if let Some(cap_name) = symbols.name(arena.get(cap.binding).name) {
                            if cap_name == target {
                                let mut fields = BTreeMap::new();
                                if let Some(fn_name) = symbols.name(arena.get(*binding).name) {
                                    fields.insert(kw("name"), Value::string(fn_name));
                                }
                                fields.insert(kw("line"), Value::int(value.span.line as i64));
                                results.push(Value::struct_from(fields));
                                break;
                            }
                        }
                    }
                }
                find_capturers(value, arena, symbols, target, results);
            }
            find_capturers(body, arena, symbols, target, results);
        }
        HirKind::Define { binding, value } => {
            if let HirKind::Lambda { captures, .. } = &value.kind {
                for cap in captures {
                    if let Some(cap_name) = symbols.name(arena.get(cap.binding).name) {
                        if cap_name == target {
                            let mut fields = BTreeMap::new();
                            if let Some(fn_name) = symbols.name(arena.get(*binding).name) {
                                fields.insert(kw("name"), Value::string(fn_name));
                            }
                            fields.insert(kw("line"), Value::int(value.span.line as i64));
                            results.push(Value::struct_from(fields));
                            break;
                        }
                    }
                }
            }
            find_capturers(value, arena, symbols, target, results);
        }
        HirKind::Lambda { body, .. } => find_capturers(body, arena, symbols, target, results),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            find_capturers(cond, arena, symbols, target, results);
            find_capturers(then_branch, arena, symbols, target, results);
            find_capturers(else_branch, arena, symbols, target, results);
        }
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                find_capturers(e, arena, symbols, target, results);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                find_capturers(e, arena, symbols, target, results);
            }
        }
        HirKind::Call { func, args, .. } => {
            find_capturers(func, arena, symbols, target, results);
            for arg in args {
                find_capturers(&arg.expr, arena, symbols, target, results);
            }
        }
        HirKind::Assign { value, .. } => find_capturers(value, arena, symbols, target, results),
        HirKind::While { cond, body } => {
            find_capturers(cond, arena, symbols, target, results);
            find_capturers(body, arena, symbols, target, results);
        }
        HirKind::Match { value, arms } => {
            find_capturers(value, arena, symbols, target, results);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    find_capturers(g, arena, symbols, target, results);
                }
                find_capturers(body, arena, symbols, target, results);
            }
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            find_capturers(e, arena, symbols, target, results);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                find_capturers(c, arena, symbols, target, results);
                find_capturers(b, arena, symbols, target, results);
            }
            if let Some(e) = else_branch {
                find_capturers(e, arena, symbols, target, results);
            }
        }
        HirKind::Destructure { value, .. } => {
            find_capturers(value, arena, symbols, target, results)
        }
        HirKind::Eval { expr, env } => {
            find_capturers(expr, arena, symbols, target, results);
            find_capturers(env, arena, symbols, target, results);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                find_capturers(p, arena, symbols, target, results);
                find_capturers(v, arena, symbols, target, results);
            }
            find_capturers(body, arena, symbols, target, results);
        }
        _ => {}
    }
}

/// (compile/callers analysis :name) → [{:name "main" :line 50 :tail false}]
pub(super) fn prim_compile_callers(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/callers") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/callers") {
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
            fields.insert(kw("name"), Value::string(&**caller_name));
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
            Value::struct_from(fields)
        })
        .collect();

    (SIG_OK, Value::array(values))
}

/// (compile/callees analysis :name) → [{:name "http/get" :line 3 :tail false}]
pub(super) fn prim_compile_callees(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/callees") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/callees") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let edges = handle
        .call_graph
        .edges
        .get(&name)
        .cloned()
        .unwrap_or_default();

    let values: Vec<Value> = edges.iter().map(call_edge_to_value).collect();
    (SIG_OK, Value::array(values))
}

/// (compile/call-graph analysis) → {:nodes [...] :roots [...] :leaves [...]}
pub(super) fn prim_compile_call_graph(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/call-graph") {
        Ok(h) => h,
        Err(e) => return e,
    };

    let nodes: Vec<Value> = handle
        .call_graph
        .edges
        .iter()
        .map(|(name, edges)| {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), Value::string(&**name));
            fields.insert(
                kw("callees"),
                Value::array(edges.iter().map(|e| Value::string(&*e.callee)).collect()),
            );
            let callers = handle
                .call_graph
                .reverse
                .get(name)
                .cloned()
                .unwrap_or_default();
            fields.insert(
                kw("callers"),
                Value::array(callers.iter().map(|c| Value::string(&**c)).collect()),
            );
            Value::struct_from(fields)
        })
        .collect();

    let mut fields = BTreeMap::new();
    fields.insert(kw("nodes"), Value::array(nodes));
    fields.insert(
        kw("roots"),
        Value::array(
            handle
                .call_graph
                .roots
                .iter()
                .map(|s| Value::string(&**s))
                .collect(),
        ),
    );
    fields.insert(
        kw("leaves"),
        Value::array(
            handle
                .call_graph
                .leaves
                .iter()
                .map(|s| Value::string(&**s))
                .collect(),
        ),
    );

    (SIG_OK, Value::struct_from(fields))
}

/// (compile/binding analysis :name) → {:scope :local :mutated true ...}
pub(super) fn prim_compile_binding(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/binding") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/binding") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/binding: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Find the binding by name.
    for i in 0..handle.arena.len() {
        let binding = crate::hir::Binding(i as u32);
        let inner = handle.arena.get(binding);
        if let Some(bname) = symbols.name(inner.name) {
            if bname == name {
                let mut fields = BTreeMap::new();
                fields.insert(kw("name"), Value::string(bname));
                fields.insert(
                    kw("scope"),
                    Value::keyword(match inner.scope {
                        crate::hir::arena::BindingScope::Parameter => "parameter",
                        crate::hir::arena::BindingScope::Local => "local",
                    }),
                );
                fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
                fields.insert(kw("captured"), Value::bool(inner.is_captured));
                fields.insert(kw("immutable"), Value::bool(inner.is_immutable));
                fields.insert(kw("needs-lbox"), Value::bool(inner.needs_capture()));

                if let Some(loc) = handle.symbol_index.symbol_locations.get(&inner.name) {
                    fields.insert(kw("line"), Value::int(loc.line as i64));
                    fields.insert(kw("col"), Value::int(loc.col as i64));
                }

                // Usages.
                if let Some(usages) = handle.symbol_index.symbol_usages.get(&inner.name) {
                    let usage_vals: Vec<Value> = usages
                        .iter()
                        .map(|loc| {
                            let mut f = BTreeMap::new();
                            f.insert(kw("line"), Value::int(loc.line as i64));
                            f.insert(kw("col"), Value::int(loc.col as i64));
                            Value::struct_from(f)
                        })
                        .collect();
                    fields.insert(kw("usages"), Value::array(usage_vals));
                }

                return (SIG_OK, Value::struct_from(fields));
            }
        }
    }

    (
        SIG_ERROR,
        error_val(
            "lookup-error",
            format!("compile/binding: no binding '{}' in analysis", name),
        ),
    )
}

// ── Primitive metadata ─────────────────────────────────────────────────

/// Return metadata for all Rust-defined primitives as an array of structs.
///
/// Each struct: {:name :category :arity :signal :doc :params :aliases}
pub(super) fn prim_compile_primitives(args: &[Value]) -> (SignalBits, Value) {
    let _ = args;
    use crate::primitives::registration::ALL_TABLES;

    let mut results = Vec::new();

    for table in ALL_TABLES {
        for def in *table {
            let mut fields = BTreeMap::new();
            fields.insert(kw("name"), Value::string(def.name));
            fields.insert(
                kw("category"),
                if def.category.is_empty() {
                    Value::string("core")
                } else {
                    Value::string(def.category)
                },
            );
            fields.insert(kw("arity"), Value::string(format!("{}", def.arity)));
            fields.insert(kw("signal"), signal_to_value(&def.signal));
            fields.insert(kw("doc"), Value::string(def.doc));

            let params: Vec<Value> = def.params.iter().map(|p| Value::string(*p)).collect();
            fields.insert(kw("params"), Value::array(params));

            let aliases: Vec<Value> = def.aliases.iter().map(|a| Value::string(*a)).collect();
            fields.insert(kw("aliases"), Value::array(aliases));

            results.push(Value::struct_from(fields));
        }
    }

    (SIG_OK, Value::array(results))
}
