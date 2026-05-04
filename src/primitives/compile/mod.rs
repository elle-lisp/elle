//! Compiler-as-library primitives: analyze Elle source and query the results.
//!
//! The `compile/analyze` primitive runs the full analysis pipeline (reader →
//! expander → analyzer) and returns an opaque handle.  Other `compile/*`
//! primitives accept the handle and extract structured views: signals,
//! bindings, captures, call graph, diagnostics, symbols.

pub(super) mod query;
pub(super) mod transform;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::hir::BindingArena;
use crate::hir::{Binding, Hir, HirKind};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::primitives::def::PrimitiveDef;
use crate::signals::registry::with_registry;
use crate::signals::Signal;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use crate::value::error_val;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_QUERY};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;

// ── Helper ─────────────────────────────────────────────────────────────

pub(super) fn kw(name: &str) -> TableKey {
    TableKey::Keyword(name.to_string())
}

// ── Analysis handle ────────────────────────────────────────────────────

/// Opaque handle wrapping the result of `analyze_file`.
///
/// Stored as `Value::external("analysis", AnalysisHandle)`.  Query
/// primitives downcast the External to access the fields.
/// (byte_offset, byte_len) of a name token in source text.
pub(super) type NameSpan = (usize, usize);

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

pub(super) fn build_signal_map(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> HashMap<String, Signal> {
    let mut map = HashMap::new();
    collect_fn_signals(hir, arena, symbols, &mut map);
    map
}

pub(super) fn collect_fn_signals(
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
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                collect_fn_signals(init, arena, symbols, map);
            }
            collect_fn_signals(body, arena, symbols, map);
        }
        HirKind::Recur { args } => {
            for arg in args {
                collect_fn_signals(arg, arena, symbols, map);
            }
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
        HirKind::Emit { value: expr, .. } | HirKind::Break { value: expr, .. } => {
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
        HirKind::MakeCell { value } => {
            collect_fn_signals(value, arena, symbols, map);
        }
        HirKind::DerefCell { cell } => {
            collect_fn_signals(cell, arena, symbols, map);
        }
        HirKind::SetCell { cell, value } => {
            collect_fn_signals(cell, arena, symbols, map);
            collect_fn_signals(value, arena, symbols, map);
        }
        HirKind::Intrinsic { args, .. } => {
            for a in args {
                collect_fn_signals(a, arena, symbols, map);
            }
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

        HirKind::Error => {}
    }
}

// ── Call graph builder ─────────────────────────────────────────────────

pub(super) fn build_call_graph(
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

pub(super) fn collect_call_edges(
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
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                collect_call_edges(init, arena, symbols, edges, current_fn);
            }
            collect_call_edges(body, arena, symbols, edges, current_fn);
        }
        HirKind::Recur { args } => {
            for arg in args {
                collect_call_edges(arg, arena, symbols, edges, current_fn);
            }
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
        HirKind::Emit { value: expr, .. } | HirKind::Break { value: expr, .. } => {
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
        HirKind::MakeCell { value } => {
            collect_call_edges(value, arena, symbols, edges, current_fn);
        }
        HirKind::DerefCell { cell } => {
            collect_call_edges(cell, arena, symbols, edges, current_fn);
        }
        HirKind::SetCell { cell, value } => {
            collect_call_edges(cell, arena, symbols, edges, current_fn);
            collect_call_edges(value, arena, symbols, edges, current_fn);
        }
        HirKind::Intrinsic { args, .. } => {
            for a in args {
                collect_call_edges(a, arena, symbols, edges, current_fn);
            }
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

        HirKind::Error => {}
    }
}

// ── Binding spans builder ──────────────────────────────────────────────

/// Check if a byte can appear in an Elle identifier token.
pub(super) fn is_ident_byte(b: u8) -> bool {
    !b.is_ascii_whitespace()
        && !matches!(
            b,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'#' | b'"' | b'\''
        )
}

/// Find the first occurrence of `name` as a standalone token in `source[start..end]`.
/// Returns `(absolute_byte_offset, byte_len)`.
pub(super) fn find_name_in_span(
    source: &str,
    start: usize,
    end: usize,
    name: &str,
) -> Option<NameSpan> {
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
pub(super) fn build_binding_spans(
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
pub(super) fn find_named_lambda<'a>(
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
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
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
pub(super) fn collect_vars_in_range(
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
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
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
pub(super) fn compute_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

/// Find the matching close paren for an open paren at `start`.
pub(super) fn find_matching_paren(source: &str, start: usize) -> Option<usize> {
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

pub(super) fn signal_to_value(sig: &Signal) -> Value {
    let mut fields = BTreeMap::new();

    // :bits as keyword set
    let bit_set = with_registry(|reg| {
        let mut bit_set = BTreeSet::new();
        for entry in reg.entries() {
            if sig.bits.has_bit(entry.bit_position) {
                bit_set.insert(Value::keyword(&entry.name));
            }
        }
        bit_set
    });
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
    let silent = sig.bits.is_empty() && sig.propagates == 0;
    let yields = sig.may_suspend();
    let io = sig.bits.contains(crate::signals::SIG_IO);
    fields.insert(kw("silent"), Value::bool(silent));
    fields.insert(kw("yields"), Value::bool(yields));
    fields.insert(kw("io"), Value::bool(io));
    fields.insert(kw("jit-eligible"), Value::bool(!yields));

    Value::struct_from(fields)
}

pub(super) fn diagnostic_to_value(d: &Diagnostic) -> Value {
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

pub(super) fn symbol_def_to_value(def: &SymbolDef) -> Value {
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

pub(super) fn call_edge_to_value(edge: &CallEdge) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("name"), Value::string(&*edge.callee));
    fields.insert(kw("line"), Value::int(edge.line as i64));
    fields.insert(kw("col"), Value::int(edge.col as i64));
    fields.insert(kw("tail"), Value::bool(edge.is_tail));
    Value::struct_from(fields)
}

// ── Extract the handle from an argument ────────────────────────────────

pub(super) fn get_handle<'a>(
    args: &'a [Value],
    name: &str,
) -> Result<&'a AnalysisHandle, (SignalBits, Value)> {
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
pub(super) fn resolve_name(
    args: &[Value],
    idx: usize,
    prim_name: &str,
) -> Result<String, (SignalBits, Value)> {
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

// ── Registration ───────────────────────────────────────────────────────

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "compile/analyze",
        func: query::prim_compile_analyze,
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
        func: query::prim_compile_diagnostics,
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
        func: query::prim_compile_symbols,
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
        func: query::prim_compile_signal,
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
        func: query::prim_compile_query_signal,
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
        func: query::prim_compile_bindings,
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
        func: query::prim_compile_binding,
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
        func: query::prim_compile_captures,
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
        func: query::prim_compile_captured_by,
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
        func: query::prim_compile_callers,
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
        func: query::prim_compile_callees,
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
        func: query::prim_compile_call_graph,
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
        func: query::prim_compile_primitives,
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
        func: transform::prim_compile_rename,
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
        func: transform::prim_compile_extract,
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
        func: transform::prim_compile_parallelize,
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
        func: transform::prim_compile_add_handler,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Wrap call sites of a function with signal handling.",
        params: &["analysis", "fn-name", "signal-kind"],
        category: "compile",
        example: r#"(compile/add-handler analysis :fetch-page :error)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compile/run-on",
        func: transform::prim_compile_run_on,
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
        arity: Arity::AtLeast(2),
        doc: "Force-dispatch a closure on a specific tier (:bytecode, :jit, :mlir-cpu). Used by lib/differential.lisp to verify tier agreement. Returns the result, or signals :tier-rejected if the tier doesn't accept the closure.",
        params: &["tier", "f"],
        category: "compile",
        example: r#"(compile/run-on :bytecode (fn [a b] (+ a b)) 3 4)"#,
        aliases: &[],
    },
];
