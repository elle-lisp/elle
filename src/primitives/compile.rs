//! Compiler-as-library primitives: analyze Elle source and query the results.
//!
//! The `compile/analyze` primitive runs the full analysis pipeline (reader →
//! expander → analyzer) and returns an opaque handle.  Other `compile/*`
//! primitives accept the handle and extract structured views: signals,
//! bindings, captures, call graph, diagnostics, symbols.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::context;
use crate::hir::symbols::extract_symbols_from_hir;
use crate::hir::{Binding, Hir, HirKind};
use crate::hir::{BindingArena, HirLinter};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::pipeline::analyze_file;
use crate::primitives::def::PrimitiveDef;
use crate::rewrite::edit::{apply_edits, Edit};
use crate::signals::registry::global_registry;
use crate::signals::Signal;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use crate::value::error_val;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;

// ── Helper ─────────────────────────────────────────────────────────────

fn kw(name: &str) -> TableKey {
    TableKey::Keyword(name.to_string())
}

// ── Analysis handle ────────────────────────────────────────────────────

/// Opaque handle wrapping the result of `analyze_file`.
///
/// Stored as `Value::external("analysis", AnalysisHandle)`.  Query
/// primitives downcast the External to access the fields.
/// (byte_offset, byte_len) of a name token in source text.
type NameSpan = (usize, usize);

pub struct AnalysisHandle {
    pub hir: Hir,
    pub arena: BindingArena,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
    /// Function name → Signal, built eagerly.
    pub signal_map: HashMap<String, Signal>,
    /// Function name → `Vec<CallEdge>`, built eagerly.
    pub call_graph: CallGraphData,
    /// Original source text.
    pub source: String,
    /// Binding → all source locations where the binding's name appears.
    pub binding_spans: HashMap<Binding, Vec<NameSpan>>,
}

#[derive(Debug, Clone)]
pub struct CallEdge {
    pub callee: String,
    pub line: u32,
    pub col: u32,
    pub is_tail: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CallGraphData {
    /// caller name → outgoing edges
    pub edges: HashMap<String, Vec<CallEdge>>,
    /// callee name → caller names
    pub reverse: HashMap<String, Vec<String>>,
    /// Functions with no callers.
    pub roots: Vec<String>,
    /// Functions that call no user-defined functions.
    pub leaves: Vec<String>,
}

// ── Signal map builder ─────────────────────────────────────────────────

fn build_signal_map(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> HashMap<String, Signal> {
    let mut map = HashMap::new();
    collect_fn_signals(hir, arena, symbols, &mut map);
    map
}

fn collect_fn_signals(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    map: &mut HashMap<String, Signal>,
) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let HirKind::Lambda {
                    inferred_signals, ..
                } = &value.kind
                {
                    if let Some(name) = symbols.name(arena.get(*binding).name) {
                        map.insert(name.to_string(), *inferred_signals);
                    }
                }
                collect_fn_signals(value, arena, symbols, map);
            }
            collect_fn_signals(body, arena, symbols, map);
        }
        HirKind::Define { binding, value } => {
            if let HirKind::Lambda {
                inferred_signals, ..
            } = &value.kind
            {
                if let Some(name) = symbols.name(arena.get(*binding).name) {
                    map.insert(name.to_string(), *inferred_signals);
                }
            }
            collect_fn_signals(value, arena, symbols, map);
        }
        HirKind::Lambda { body, .. } => {
            collect_fn_signals(body, arena, symbols, map);
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_fn_signals(cond, arena, symbols, map);
            collect_fn_signals(then_branch, arena, symbols, map);
            collect_fn_signals(else_branch, arena, symbols, map);
        }
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                collect_fn_signals(e, arena, symbols, map);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                collect_fn_signals(e, arena, symbols, map);
            }
        }
        HirKind::Call { func, args, .. } => {
            collect_fn_signals(func, arena, symbols, map);
            for arg in args {
                collect_fn_signals(&arg.expr, arena, symbols, map);
            }
        }
        HirKind::Assign { value, .. } => {
            collect_fn_signals(value, arena, symbols, map);
        }
        HirKind::While { cond, body } => {
            collect_fn_signals(cond, arena, symbols, map);
            collect_fn_signals(body, arena, symbols, map);
        }
        HirKind::Match { value, arms } => {
            collect_fn_signals(value, arena, symbols, map);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    collect_fn_signals(g, arena, symbols, map);
                }
                collect_fn_signals(body, arena, symbols, map);
            }
        }
        HirKind::Yield(expr) | HirKind::Break { value: expr, .. } => {
            collect_fn_signals(expr, arena, symbols, map);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                collect_fn_signals(c, arena, symbols, map);
                collect_fn_signals(b, arena, symbols, map);
            }
            if let Some(e) = else_branch {
                collect_fn_signals(e, arena, symbols, map);
            }
        }
        HirKind::Destructure { value, .. } => {
            collect_fn_signals(value, arena, symbols, map);
        }
        HirKind::Eval { expr, env } => {
            collect_fn_signals(expr, arena, symbols, map);
            collect_fn_signals(env, arena, symbols, map);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                collect_fn_signals(p, arena, symbols, map);
                collect_fn_signals(v, arena, symbols, map);
            }
            collect_fn_signals(body, arena, symbols, map);
        }
        // Leaves: no children to recurse into.
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_) => {}
    }
}

// ── Call graph builder ─────────────────────────────────────────────────

fn build_call_graph(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    signal_map: &HashMap<String, Signal>,
) -> CallGraphData {
    let mut edges: HashMap<String, Vec<CallEdge>> = HashMap::new();

    // Walk HIR, tracking the current enclosing function name.
    collect_call_edges(hir, arena, symbols, &mut edges, None);

    // Build reverse map.
    let mut reverse: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_callees: BTreeSet<String> = BTreeSet::new();
    let mut all_callers: BTreeSet<String> = BTreeSet::new();

    for (caller, callee_edges) in &edges {
        all_callers.insert(caller.clone());
        for edge in callee_edges {
            all_callees.insert(edge.callee.clone());
            reverse
                .entry(edge.callee.clone())
                .or_default()
                .push(caller.clone());
        }
    }

    // Roots: defined functions with no callers.
    let defined: BTreeSet<String> = signal_map.keys().cloned().collect();
    let roots: Vec<String> = defined
        .iter()
        .filter(|name| !all_callees.contains(*name))
        .cloned()
        .collect();

    // Leaves: defined functions that call no other user-defined functions.
    let leaves: Vec<String> = defined
        .iter()
        .filter(|name| edges.get(*name).map(|e| e.is_empty()).unwrap_or(true))
        .cloned()
        .collect();

    CallGraphData {
        edges,
        reverse,
        roots,
        leaves,
    }
}

fn collect_call_edges(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    edges: &mut HashMap<String, Vec<CallEdge>>,
    current_fn: Option<&str>,
) {
    match &hir.kind {
        // Track the current function context via Define/Letrec bindings.
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                let name = symbols.name(arena.get(*binding).name);
                if matches!(value.kind, HirKind::Lambda { .. }) {
                    let fn_name = name.map(|n| n.to_string());
                    let ctx = fn_name.as_deref().or(current_fn);
                    collect_call_edges(value, arena, symbols, edges, ctx);
                } else {
                    collect_call_edges(value, arena, symbols, edges, current_fn);
                }
            }
            collect_call_edges(body, arena, symbols, edges, current_fn);
        }
        HirKind::Define { binding, value } => {
            let name = symbols.name(arena.get(*binding).name);
            if matches!(value.kind, HirKind::Lambda { .. }) {
                let fn_name = name.map(|n| n.to_string());
                let ctx = fn_name.as_deref().or(current_fn);
                collect_call_edges(value, arena, symbols, edges, ctx);
            } else {
                collect_call_edges(value, arena, symbols, edges, current_fn);
            }
        }

        // Record call edges.
        HirKind::Call {
            func,
            args,
            is_tail,
        } => {
            if let Some(caller) = current_fn {
                if let HirKind::Var(binding) = &func.kind {
                    if let Some(callee_name) = symbols.name(arena.get(*binding).name) {
                        edges.entry(caller.to_string()).or_default().push(CallEdge {
                            callee: callee_name.to_string(),
                            line: hir.span.line,
                            col: hir.span.col,
                            is_tail: *is_tail,
                        });
                    }
                }
            }
            collect_call_edges(func, arena, symbols, edges, current_fn);
            for arg in args {
                collect_call_edges(&arg.expr, arena, symbols, edges, current_fn);
            }
        }

        // Don't descend into nested lambdas — they're their own function
        // context.  We DO descend, but without the parent's current_fn.
        HirKind::Lambda { body, .. } => {
            // If we reached here, this is an anonymous lambda not bound
            // via Define/Letrec — descend without function context.
            collect_call_edges(body, arena, symbols, edges, current_fn);
        }

        // Recurse into all other forms.
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_call_edges(cond, arena, symbols, edges, current_fn);
            collect_call_edges(then_branch, arena, symbols, edges, current_fn);
            collect_call_edges(else_branch, arena, symbols, edges, current_fn);
        }
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                collect_call_edges(e, arena, symbols, edges, current_fn);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                collect_call_edges(e, arena, symbols, edges, current_fn);
            }
        }
        HirKind::Assign { value, .. } => {
            collect_call_edges(value, arena, symbols, edges, current_fn);
        }
        HirKind::While { cond, body } => {
            collect_call_edges(cond, arena, symbols, edges, current_fn);
            collect_call_edges(body, arena, symbols, edges, current_fn);
        }
        HirKind::Match { value, arms } => {
            collect_call_edges(value, arena, symbols, edges, current_fn);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    collect_call_edges(g, arena, symbols, edges, current_fn);
                }
                collect_call_edges(body, arena, symbols, edges, current_fn);
            }
        }
        HirKind::Yield(expr) | HirKind::Break { value: expr, .. } => {
            collect_call_edges(expr, arena, symbols, edges, current_fn);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                collect_call_edges(c, arena, symbols, edges, current_fn);
                collect_call_edges(b, arena, symbols, edges, current_fn);
            }
            if let Some(e) = else_branch {
                collect_call_edges(e, arena, symbols, edges, current_fn);
            }
        }
        HirKind::Destructure { value, .. } => {
            collect_call_edges(value, arena, symbols, edges, current_fn);
        }
        HirKind::Eval { expr, env } => {
            collect_call_edges(expr, arena, symbols, edges, current_fn);
            collect_call_edges(env, arena, symbols, edges, current_fn);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                collect_call_edges(p, arena, symbols, edges, current_fn);
                collect_call_edges(v, arena, symbols, edges, current_fn);
            }
            collect_call_edges(body, arena, symbols, edges, current_fn);
        }
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_) => {}
    }
}

// ── Binding spans builder ──────────────────────────────────────────────

/// Check if a byte can appear in an Elle identifier token.
fn is_ident_byte(b: u8) -> bool {
    !b.is_ascii_whitespace()
        && !matches!(
            b,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'#' | b'"' | b'\''
        )
}

/// Find the first occurrence of `name` as a standalone token in `source[start..end]`.
/// Returns `(absolute_byte_offset, byte_len)`.
fn find_name_in_span(source: &str, start: usize, end: usize, name: &str) -> Option<NameSpan> {
    if start >= end || name.is_empty() || end > source.len() || start > source.len() {
        return None;
    }
    // Clamp to valid source range.
    let end = end.min(source.len());
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return None;
    }
    let region = &source.as_bytes()[start..end];
    let nb = name.as_bytes();
    let nlen = nb.len();
    if nlen > region.len() {
        return None;
    }
    for i in 0..=(region.len() - nlen) {
        if &region[i..i + nlen] == nb {
            let before_ok = i == 0 || !is_ident_byte(region[i - 1]);
            let after_ok = i + nlen >= region.len() || !is_ident_byte(region[i + nlen]);
            if before_ok && after_ok {
                return Some((start + i, nlen));
            }
        }
    }
    None
}

/// Build binding spans using symbol_index data (line/col → byte offsets).
/// HIR byte spans are unreliable for macro-expanded code, so we use the
/// symbol_index which records definition and usage locations correctly.
fn build_binding_spans(
    _hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    source: &str,
    symbol_index: &SymbolIndex,
    spans: &mut HashMap<Binding, Vec<NameSpan>>,
) {
    let line_offsets = compute_line_offsets(source);

    for i in 0..arena.len() {
        let binding = Binding(i as u32);
        let inner = arena.get(binding);
        let name = match symbols.name(inner.name) {
            Some(n) => n,
            None => continue,
        };

        // Definition site from symbol_locations.
        if let Some(loc) = symbol_index.symbol_locations.get(&inner.name) {
            if loc.line > 0 {
                if let Some(&line_start) = line_offsets.get(loc.line - 1) {
                    let byte_start = line_start + loc.col.saturating_sub(1);
                    if byte_start < source.len() {
                        let search_end = (byte_start + name.len() + 20).min(source.len());
                        if let Some(ns) = find_name_in_span(source, byte_start, search_end, name) {
                            spans.entry(binding).or_default().push(ns);
                        }
                    }
                }
            }
        }

        // Usage sites from symbol_usages (deduplicated against definition site).
        if let Some(usages) = symbol_index.symbol_usages.get(&inner.name) {
            for usage in usages {
                if usage.line > 0 {
                    if let Some(&line_start) = line_offsets.get(usage.line - 1) {
                        let byte_start = line_start + usage.col.saturating_sub(1);
                        if byte_start >= source.len() {
                            continue;
                        }
                        let search_end = (byte_start + name.len() + 10).min(source.len());
                        if let Some(ns) = find_name_in_span(source, byte_start, search_end, name) {
                            let entry = spans.entry(binding).or_default();
                            if !entry.contains(&ns) {
                                entry.push(ns);
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── HIR search helpers ────────────────────────────────────────────────

/// Find the Lambda Hir node for a named function definition.
fn find_named_lambda<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
) -> Option<&'a Hir> {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let Some(name) = symbols.name(arena.get(*binding).name) {
                    if name == target && matches!(value.kind, HirKind::Lambda { .. }) {
                        return Some(value);
                    }
                }
                if let Some(r) = find_named_lambda(value, arena, symbols, target) {
                    return Some(r);
                }
            }
            find_named_lambda(body, arena, symbols, target)
        }
        HirKind::Define { binding, value } => {
            if let Some(name) = symbols.name(arena.get(*binding).name) {
                if name == target && matches!(value.kind, HirKind::Lambda { .. }) {
                    return Some(value);
                }
            }
            find_named_lambda(value, arena, symbols, target)
        }
        HirKind::Lambda { body, .. } => find_named_lambda(body, arena, symbols, target),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_named_lambda(cond, arena, symbols, target)
            .or_else(|| find_named_lambda(then_branch, arena, symbols, target))
            .or_else(|| find_named_lambda(else_branch, arena, symbols, target)),
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => {
            for e in es {
                if let Some(r) = find_named_lambda(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Block { body, .. } => {
            for e in body {
                if let Some(r) = find_named_lambda(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Call { func, args, .. } => find_named_lambda(func, arena, symbols, target)
            .or_else(|| {
                for a in args {
                    if let Some(r) = find_named_lambda(&a.expr, arena, symbols, target) {
                        return Some(r);
                    }
                }
                None
            }),
        HirKind::Assign { value, .. } => find_named_lambda(value, arena, symbols, target),
        HirKind::While { cond, body } => find_named_lambda(cond, arena, symbols, target)
            .or_else(|| find_named_lambda(body, arena, symbols, target)),
        HirKind::Match { value, arms } => {
            find_named_lambda(value, arena, symbols, target).or_else(|| {
                for (_, g, b) in arms {
                    if let Some(g) = g {
                        if let Some(r) = find_named_lambda(g, arena, symbols, target) {
                            return Some(r);
                        }
                    }
                    if let Some(r) = find_named_lambda(b, arena, symbols, target) {
                        return Some(r);
                    }
                }
                None
            })
        }
        HirKind::Yield(e) | HirKind::Break { value: e, .. } => {
            find_named_lambda(e, arena, symbols, target)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                if let Some(r) = find_named_lambda(c, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_named_lambda(b, arena, symbols, target) {
                    return Some(r);
                }
            }
            else_branch
                .as_ref()
                .and_then(|e| find_named_lambda(e, arena, symbols, target))
        }
        HirKind::Destructure { value, .. } => find_named_lambda(value, arena, symbols, target),
        HirKind::Eval { expr, env } => find_named_lambda(expr, arena, symbols, target)
            .or_else(|| find_named_lambda(env, arena, symbols, target)),
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                if let Some(r) = find_named_lambda(p, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_named_lambda(v, arena, symbols, target) {
                    return Some(r);
                }
            }
            find_named_lambda(body, arena, symbols, target)
        }
        _ => None,
    }
}

/// Collect referenced and defined bindings within a byte range of the HIR.
fn collect_vars_in_range(
    hir: &Hir,
    start: usize,
    end: usize,
    referenced: &mut BTreeSet<Binding>,
    defined: &mut BTreeSet<Binding>,
    signal: &mut Signal,
) {
    if hir.span.start >= end || hir.span.end <= start {
        return;
    }
    *signal = signal.combine(hir.signal);
    match &hir.kind {
        HirKind::Var(b) => {
            referenced.insert(*b);
        }
        HirKind::Define { binding, value } => {
            defined.insert(*binding);
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                defined.insert(*b);
                collect_vars_in_range(init, start, end, referenced, defined, signal);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Lambda {
            params,
            rest_param,
            body,
            ..
        } => {
            for p in params {
                defined.insert(*p);
            }
            if let Some(r) = rest_param {
                defined.insert(*r);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Assign { target, value } => {
            referenced.insert(*target);
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_vars_in_range(cond, start, end, referenced, defined, signal);
            collect_vars_in_range(then_branch, start, end, referenced, defined, signal);
            collect_vars_in_range(else_branch, start, end, referenced, defined, signal);
        }
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => {
            for e in es {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Call { func, args, .. } => {
            collect_vars_in_range(func, start, end, referenced, defined, signal);
            for a in args {
                collect_vars_in_range(&a.expr, start, end, referenced, defined, signal);
            }
        }
        HirKind::While { cond, body } => {
            collect_vars_in_range(cond, start, end, referenced, defined, signal);
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Match { value, arms } => {
            collect_vars_in_range(value, start, end, referenced, defined, signal);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    collect_vars_in_range(g, start, end, referenced, defined, signal);
                }
                collect_vars_in_range(b, start, end, referenced, defined, signal);
            }
        }
        HirKind::Yield(e) | HirKind::Break { value: e, .. } => {
            collect_vars_in_range(e, start, end, referenced, defined, signal);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                collect_vars_in_range(c, start, end, referenced, defined, signal);
                collect_vars_in_range(b, start, end, referenced, defined, signal);
            }
            if let Some(e) = else_branch {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Destructure { value, .. } => {
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::Eval { expr, env } => {
            collect_vars_in_range(expr, start, end, referenced, defined, signal);
            collect_vars_in_range(env, start, end, referenced, defined, signal);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                collect_vars_in_range(p, start, end, referenced, defined, signal);
                collect_vars_in_range(v, start, end, referenced, defined, signal);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        _ => {}
    }
}

/// Compute byte offsets for each line start (0-indexed lines).
fn compute_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

/// Find the matching close paren for an open paren at `start`.
fn find_matching_paren(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if !in_string => in_string = true,
            b'"' if in_string => in_string = false,
            b'\\' if in_string => {
                i += 1;
            }
            b'(' if !in_string => depth += 1,
            b')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ── Value conversion helpers ───────────────────────────────────────────

fn signal_to_value(sig: &Signal) -> Value {
    let registry = global_registry().lock().unwrap();
    let mut fields = BTreeMap::new();

    // :bits as keyword set
    let mut bit_set = BTreeSet::new();
    for entry in registry.entries() {
        if sig.bits.0 & (1 << entry.bit_position) != 0 {
            bit_set.insert(Value::keyword(&entry.name));
        }
    }
    fields.insert(kw("bits"), Value::set(bit_set));

    // :propagates as integer set
    let mut prop_set = BTreeSet::new();
    for i in 0..32u32 {
        if sig.propagates & (1 << i) != 0 {
            prop_set.insert(Value::int(i as i64));
        }
    }
    fields.insert(kw("propagates"), Value::set(prop_set));

    // Derived convenience booleans
    let silent = sig.bits.0 == 0 && sig.propagates == 0;
    let yields = sig.may_suspend();
    let io = sig.bits.0 & (1 << 9) != 0; // SIG_IO
    fields.insert(kw("silent"), Value::bool(silent));
    fields.insert(kw("yields"), Value::bool(yields));
    fields.insert(kw("io"), Value::bool(io));
    fields.insert(kw("jit-eligible"), Value::bool(!yields));

    Value::struct_from(fields)
}

fn diagnostic_to_value(d: &Diagnostic) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        kw("severity"),
        Value::keyword(match d.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }),
    );
    fields.insert(kw("code"), Value::string(&*d.code));
    fields.insert(kw("rule"), Value::string(&*d.rule));
    fields.insert(kw("message"), Value::string(&*d.message));
    if let Some(loc) = &d.location {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }
    fields.insert(
        kw("suggestions"),
        Value::array(d.suggestions.iter().map(|s| Value::string(&**s)).collect()),
    );
    Value::struct_from(fields)
}

fn symbol_def_to_value(def: &SymbolDef) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("name"), Value::string(&*def.name));
    fields.insert(
        kw("kind"),
        Value::keyword(match def.kind {
            SymbolKind::Function => "function",
            SymbolKind::Variable => "variable",
            SymbolKind::Builtin => "builtin",
            SymbolKind::Macro => "macro",
            SymbolKind::Module => "module",
        }),
    );
    if let Some(loc) = &def.location {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }
    if let Some(arity) = def.arity {
        fields.insert(kw("arity"), Value::int(arity as i64));
    }
    if let Some(doc) = &def.documentation {
        fields.insert(kw("doc"), Value::string(&**doc));
    }
    Value::struct_from(fields)
}

fn call_edge_to_value(edge: &CallEdge) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("name"), Value::string(&*edge.callee));
    fields.insert(kw("line"), Value::int(edge.line as i64));
    fields.insert(kw("col"), Value::int(edge.col as i64));
    fields.insert(kw("tail"), Value::bool(edge.is_tail));
    Value::struct_from(fields)
}

// ── Extract the handle from an argument ────────────────────────────────

fn get_handle<'a>(
    args: &'a [Value],
    name: &str,
) -> Result<&'a AnalysisHandle, (SignalBits, Value)> {
    if args.is_empty() {
        return Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected at least 1 argument, got 0", name),
            ),
        ));
    }
    match args[0].as_external::<AnalysisHandle>() {
        Some(h) => Ok(h),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected analysis handle, got {}",
                    name,
                    args[0].type_name()
                ),
            ),
        )),
    }
}

/// Resolve a keyword argument to a function name string.
fn resolve_name(
    args: &[Value],
    idx: usize,
    prim_name: &str,
) -> Result<String, (SignalBits, Value)> {
    if args.len() <= idx {
        return Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "{}: expected {} arguments, got {}",
                    prim_name,
                    idx + 1,
                    args.len()
                ),
            ),
        ));
    }
    // Accept keyword or string.
    if let Some(name) = args[idx].as_keyword_name() {
        return Ok(name.to_string());
    }
    if let Some(name) = args[idx].with_string(|s| s.to_string()) {
        return Ok(name);
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected keyword or string for function name, got {}",
                prim_name,
                args[idx].type_name()
            ),
        ),
    ))
}

// ── Primitives ─────────────────────────────────────────────────────────

/// `(compile/analyze source [opts])` → analysis handle
pub(crate) fn prim_compile_analyze(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "compile/analyze: expected 1-2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }

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
            fields
                .get(&kw("file"))
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
    let diagnostics = linter.diagnostics().to_vec();

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
pub(crate) fn prim_compile_diagnostics(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/diagnostics") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let values: Vec<Value> = handle.diagnostics.iter().map(diagnostic_to_value).collect();
    (SIG_OK, Value::array(values))
}

/// (compile/symbols analysis) → [{:name "f" :kind :function ...}]
pub(crate) fn prim_compile_symbols(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_compile_signal(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_compile_query_signal(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/query-signal") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let query = match resolve_name(args, 1, "compile/query-signal") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let registry = global_registry().lock().unwrap();

    let matches: Vec<Value> = handle
        .signal_map
        .iter()
        .filter(|(_, sig)| match query.as_str() {
            "silent" => sig.bits.0 == 0 && sig.propagates == 0,
            "jit-eligible" => !sig.may_suspend(),
            "yields" => sig.may_suspend(),
            other => {
                // Look up as a signal name.
                if let Some(bit_pos) = registry.lookup(other) {
                    sig.bits.0 & (1 << bit_pos) != 0
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
        .collect();

    (SIG_OK, Value::array(matches))
}

/// (compile/bindings analysis) → [{:name "x" :scope :parameter ...}]
pub(crate) fn prim_compile_bindings(args: &[Value]) -> (SignalBits, Value) {
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
        fields.insert(kw("needs-lbox"), Value::bool(inner.needs_lbox()));

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
pub(crate) fn prim_compile_captures(args: &[Value]) -> (SignalBits, Value) {
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

fn find_lambda_captures(
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
        HirKind::Yield(e) | HirKind::Break { value: e, .. } => {
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

fn captures_to_values(
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
                    if inner.needs_lbox() {
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
pub(crate) fn prim_compile_captured_by(args: &[Value]) -> (SignalBits, Value) {
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

fn find_capturers(
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
        HirKind::Yield(e) | HirKind::Break { value: e, .. } => {
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
pub(crate) fn prim_compile_callers(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_compile_callees(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_compile_call_graph(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_compile_binding(args: &[Value]) -> (SignalBits, Value) {
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
                fields.insert(kw("needs-lbox"), Value::bool(inner.needs_lbox()));

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
fn prim_compile_primitives(args: &[Value]) -> (SignalBits, Value) {
    let _ = args;
    use super::registration::ALL_TABLES;

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

// ── Transformation primitives ──────────────────────────────────────────

/// (compile/rename analysis :old-name :new-name) → {:source "..." :edits N}
fn prim_compile_rename(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/rename") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let old_name = match resolve_name(args, 1, "compile/rename") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let new_name = match resolve_name(args, 2, "compile/rename") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/rename: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Find the first non-primitive binding matching old_name (file-scope first).
    let mut target_binding = None;
    for i in 0..handle.arena.len() {
        let binding = Binding(i as u32);
        let inner = handle.arena.get(binding);
        if let Some(name) = symbols.name(inner.name) {
            if name == old_name {
                target_binding = Some(binding);
                break;
            }
        }
    }
    let binding = match target_binding {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
                error_val(
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
            fields.insert(kw("source"), Value::string(&*new_source));
            fields.insert(kw("edits"), Value::int(count as i64));
            (SIG_OK, Value::struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("rewrite-error", format!("compile/rename: {}", e)),
        ),
    }
}

/// (compile/extract analysis {:from :fn :lines [s e] :name :new}) → {:source :new-function :captures :signal}
fn prim_compile_extract(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/extract") {
        Ok(h) => h,
        Err(e) => return e,
    };
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "compile/extract: expected 2 arguments"),
        );
    }
    let opts = match args[1].as_struct() {
        Some(f) => f,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "compile/extract: second argument must be a struct",
                ),
            )
        }
    };

    let from_name = match opts.get(&kw("from")).and_then(|v| {
        v.as_keyword_name()
            .map(|s| s.to_string())
            .or_else(|| v.with_string(|s| s.to_string()))
    }) {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "compile/extract: :from is required"),
            )
        }
    };

    let (start_line, end_line) = match opts.get(&kw("lines")).and_then(|v| v.as_array()) {
        Some(arr) if arr.len() == 2 => {
            let s = arr[0].as_int().unwrap_or(0) as u32;
            let e = arr[1].as_int().unwrap_or(0) as u32;
            (s, e)
        }
        _ => {
            return (
                SIG_ERROR,
                error_val("type-error", "compile/extract: :lines must be [start end]"),
            )
        }
    };

    let new_fn_name = match opts.get(&kw("name")).and_then(|v| {
        v.as_keyword_name()
            .map(|s| s.to_string())
            .or_else(|| v.with_string(|s| s.to_string()))
    }) {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "compile/extract: :name is required"),
            )
        }
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/extract: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    if start_line == 0 || end_line == 0 || start_line > end_line {
        return (
            SIG_ERROR,
            error_val("range-error", "compile/extract: invalid line range"),
        );
    }

    let line_offsets = compute_line_offsets(&handle.source);
    let start_byte = line_offsets
        .get((start_line - 1) as usize)
        .copied()
        .unwrap_or(0);
    let end_byte = line_offsets
        .get(end_line as usize)
        .copied()
        .unwrap_or(handle.source.len());

    // Find the lambda and collect free vars in range.
    let lambda = match find_named_lambda(&handle.hir, &handle.arena, symbols, &from_name) {
        Some(l) => l,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "lookup-error",
                    format!("compile/extract: no function '{}'", from_name),
                ),
            )
        }
    };

    let mut referenced = BTreeSet::new();
    let mut defined = BTreeSet::new();
    let mut signal = Signal::silent();

    if let HirKind::Lambda { body, .. } = &lambda.kind {
        collect_vars_in_range(
            body,
            start_byte,
            end_byte,
            &mut referenced,
            &mut defined,
            &mut signal,
        );
    }

    let free_vars: Vec<String> = referenced
        .difference(&defined)
        .filter_map(|b| {
            symbols
                .name(handle.arena.get(*b).name)
                .map(|s| s.to_string())
        })
        .collect();

    let extracted_body = handle.source[start_byte..end_byte].trim();

    let params_str = if free_vars.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", free_vars.join(" "))
    };
    let new_function = format!(
        "(defn {} {}\n  {})",
        new_fn_name, params_str, extracted_body
    );

    let replacement = if free_vars.is_empty() {
        format!("({})", new_fn_name)
    } else {
        format!("({} {})", new_fn_name, free_vars.join(" "))
    };

    // Replace extracted range with the call.
    let mut edits = vec![Edit {
        byte_offset: start_byte,
        byte_len: end_byte - start_byte,
        replacement: format!("{}\n", replacement),
    }];

    let new_source = match apply_edits(&handle.source, &mut edits) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("rewrite-error", format!("compile/extract: {}", e)),
            )
        }
    };

    let captures_val = Value::array(free_vars.iter().map(|v| Value::string(&**v)).collect());

    let mut fields = BTreeMap::new();
    fields.insert(kw("source"), Value::string(&*new_source));
    fields.insert(kw("new-function"), Value::string(&*new_function));
    fields.insert(kw("captures"), captures_val);
    fields.insert(kw("signal"), signal_to_value(&signal));
    (SIG_OK, Value::struct_from(fields))
}

/// (compile/parallelize analysis [:fn-a :fn-b]) → {:safe bool :reason "..." :code "..." :signal {...}}
fn prim_compile_parallelize(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/parallelize") {
        Ok(h) => h,
        Err(e) => return e,
    };
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "compile/parallelize: expected 2 arguments"),
        );
    }
    let fn_names: Vec<String> = match args[1].as_array() {
        Some(arr) => {
            let mut names = Vec::new();
            for v in arr {
                match v
                    .as_keyword_name()
                    .map(|s| s.to_string())
                    .or_else(|| v.with_string(|s| s.to_string()))
                {
                    Some(n) => names.push(n),
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                "compile/parallelize: names must be keywords or strings",
                            ),
                        )
                    }
                }
            }
            names
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "compile/parallelize: second argument must be an array",
                ),
            )
        }
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/parallelize: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Collect captures for each function.
    let mut all_captures: Vec<(String, Vec<(Binding, bool)>)> = Vec::new();
    let mut combined_signal = Signal::silent();

    for name in &fn_names {
        if let Some(sig) = handle.signal_map.get(name) {
            combined_signal = combined_signal.combine(*sig);
        }
        let mut caps = Vec::new();
        if let Some(lambda) = find_named_lambda(&handle.hir, &handle.arena, symbols, name) {
            if let HirKind::Lambda { captures, .. } = &lambda.kind {
                for cap in captures {
                    let inner = handle.arena.get(cap.binding);
                    caps.push((cap.binding, inner.is_mutated));
                }
            }
        }
        all_captures.push((name.clone(), caps));
    }

    // Check pairwise for shared mutable captures.
    let mut shared = Vec::new();
    for i in 0..all_captures.len() {
        for j in (i + 1)..all_captures.len() {
            for (b1, m1) in &all_captures[i].1 {
                for (b2, m2) in &all_captures[j].1 {
                    if b1 == b2 && (*m1 || *m2) {
                        if let Some(cap_name) = symbols.name(handle.arena.get(*b1).name) {
                            let mut f = BTreeMap::new();
                            f.insert(kw("name"), Value::string(cap_name));
                            let kind = if handle.arena.get(*b1).needs_lbox() {
                                "lbox"
                            } else {
                                "value"
                            };
                            f.insert(kw("kind"), Value::keyword(kind));
                            shared.push(Value::struct_from(f));
                        }
                    }
                }
            }
        }
    }

    let safe = shared.is_empty();
    let mut fields = BTreeMap::new();
    fields.insert(kw("safe"), Value::bool(safe));

    if safe {
        let fn_list = fn_names.join(" ");
        let code = format!("(ev/map (fn [f] (f)) [{}])", fn_list);
        fields.insert(
            kw("reason"),
            Value::string("No shared mutable captures between any pair."),
        );
        fields.insert(kw("code"), Value::string(&*code));
    } else {
        fields.insert(
            kw("reason"),
            Value::string("Shared mutable captures detected."),
        );
        fields.insert(kw("shared-captures"), Value::array(shared));
    }

    fields.insert(kw("signal"), signal_to_value(&combined_signal));
    (SIG_OK, Value::struct_from(fields))
}

/// (compile/add-handler analysis :fn-name :signal-kind) → {:source "..." :wraps N}
fn prim_compile_add_handler(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/add-handler") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let fn_name = match resolve_name(args, 1, "compile/add-handler") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let signal_kind = match resolve_name(args, 2, "compile/add-handler") {
        Ok(n) => n,
        Err(e) => return e,
    };

    // Verify the function emits the signal.
    let sig = match handle.signal_map.get(&fn_name) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "lookup-error",
                    format!("compile/add-handler: no function '{}'", fn_name),
                ),
            )
        }
    };

    let registry = global_registry().lock().unwrap();
    let bit = match registry.lookup(&signal_kind) {
        Some(b) => b,
        None => match signal_kind.as_str() {
            "error" => 0,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "lookup-error",
                        format!("compile/add-handler: unknown signal '{}'", signal_kind),
                    ),
                )
            }
        },
    };
    drop(registry);

    if sig.bits.0 & (1 << bit) == 0 && sig.propagates & (1 << bit) == 0 {
        return (
            SIG_ERROR,
            error_val(
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
                                    "(let [[[ok? result] (protect {})]] \
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
            fields.insert(kw("source"), Value::string(&*new_source));
            fields.insert(kw("wraps"), Value::int(wrap_count));
            (SIG_OK, Value::struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("rewrite-error", format!("compile/add-handler: {}", e)),
        ),
    }
}

// ── Registration ───────────────────────────────────────────────────────

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "compile/analyze",
        func: prim_compile_analyze,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Analyze Elle source text. Returns an opaque analysis handle for queries.",
        params: &["source", "opts"],
        category: "compile",
        example: r#"(compile/analyze "(defn f [x] (+ x 1))")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/diagnostics",
        func: prim_compile_diagnostics,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return diagnostics (warnings, errors) from an analysis.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/diagnostics (compile/analyze src))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/symbols",
        func: prim_compile_symbols,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return all symbol definitions from an analysis.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/symbols (compile/analyze src))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/signal",
        func: prim_compile_signal,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return the inferred signal of a named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/signal analysis :my-fn)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/query-signal",
        func: prim_compile_query_signal,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Find functions matching a signal query (:silent, :io, :yields, :jit-eligible, or signal name).",
        params: &["analysis", "query"],
        category: "compile",
        example: r#"(compile/query-signal analysis :silent)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/bindings",
        func: prim_compile_bindings,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return all bindings from an analysis with metadata.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/bindings (compile/analyze src))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/binding",
        func: prim_compile_binding,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return detailed info about a specific binding.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/binding analysis :x)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/captures",
        func: prim_compile_captures,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return what a function captures and how (value, lbox, transitive).",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/captures analysis :make-handler)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/captured-by",
        func: prim_compile_captured_by,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions that capture the named binding.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/captured-by analysis :config)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/callers",
        func: prim_compile_callers,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions that call the named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/callers analysis :fetch-page)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/callees",
        func: prim_compile_callees,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions called by the named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/callees analysis :main)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/call-graph",
        func: prim_compile_call_graph,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the full call graph with nodes, roots, and leaves.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/call-graph (compile/analyze src))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/primitives",
        func: prim_compile_primitives,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return metadata for all Rust-defined primitives as an array of structs.",
        params: &[],
        category: "compile",
        example: r#"(compile/primitives)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/rename",
        func: prim_compile_rename,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Binding-aware rename. Returns new source with all references updated.",
        params: &["analysis", "old-name", "new-name"],
        category: "compile",
        example: r#"(compile/rename analysis :old-name :new-name)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/extract",
        func: prim_compile_extract,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Extract a line range into a new function. Returns new source, captures, and signal.",
        params: &["analysis", "opts"],
        category: "compile",
        example: r#"(compile/extract analysis {:from :fn :lines [5 10] :name :new-fn})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/parallelize",
        func: prim_compile_parallelize,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if functions can safely run in parallel. Verifies no shared mutable captures.",
        params: &["analysis", "fn-names"],
        category: "compile",
        example: r#"(compile/parallelize analysis [:fetch-a :fetch-b])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/add-handler",
        func: prim_compile_add_handler,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Wrap call sites of a function with signal handling.",
        params: &["analysis", "fn-name", "signal-kind"],
        category: "compile",
        example: r#"(compile/add-handler analysis :fetch-page :error)"#,
        aliases: &[],
    },
];
