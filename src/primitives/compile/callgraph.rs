// audited: 2026-11-20
//! The signal map and the call graph that `compile/analyze` builds from the analyzed HIR, over what the source spells.
//!
//! docs/analysis/portrait.md

use super::*;
use crate::hir::HirId;
use std::collections::HashSet;

/// The name of the function `binding` defines when `value` is a lambda and
/// the source spelled the binding: no template wrote it, and no compiler
/// temporary is it.
pub(crate) fn defined_name<'s>(
    binding: Binding,
    value: &Hir,
    arena: &BindingArena,
    symbols: &'s crate::symbol::SymbolTable,
) -> Option<&'s str> {
    let inner = arena.get(binding);
    if inner.is_synthetic || inner.is_macro_introduced || value.kind.as_lambda().is_none() {
        return None;
    }
    symbols.name(inner.name)
}

/// Call `f` with each binding a `let`, `letrec` or `def` in `hir` makes, and
/// the value it binds.
pub(crate) fn each_definition<'a>(hir: &'a Hir, f: &mut impl FnMut(Binding, &'a Hir)) {
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (binding, value) in bindings {
                f(*binding, value);
            }
        }
        HirKind::Define { binding, value } => f(*binding, value),
        _ => {}
    }
    hir.for_each_child(|child| each_definition(child, f));
}

/// The signal of each function the source defines, by name.
pub(crate) fn build_signal_map(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> HashMap<String, Signal> {
    let mut map = HashMap::new();
    each_definition(hir, &mut |binding, value| {
        if let (Some(name), Some(lambda)) = (
            defined_name(binding, value, arena, symbols),
            value.kind.as_lambda(),
        ) {
            map.insert(name.to_string(), lambda.inferred_signals);
        }
    });
    map
}

/// The callee of a call as the source spells it: a binding's name, or a
/// qualified name rebuilt from the `get` chain it desugars to.
fn spelled(
    func: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> Option<String> {
    match &func.kind {
        HirKind::Var(binding) => symbols.name(arena.get(*binding).name).map(str::to_string),
        HirKind::Call { args, .. } => match args.as_slice() {
            [base, key] => match &key.expr.kind {
                HirKind::Keyword(segment) => Some(format!(
                    "{}:{segment}",
                    spelled(&base.expr, arena, symbols)?
                )),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// The walk that collects the calls the source spells into a call graph.
struct EdgeWalk<'a> {
    arena: &'a BindingArena,
    symbols: &'a crate::symbol::SymbolTable,
    written: &'a HashSet<HirId>,
    /// The signal of each callee binding the analysis knows: a function the
    /// unit binds and never reassigns, a primitive, a standard-library function.
    signals: HashMap<Binding, Signal>,
    /// The call sites already recorded, so the copies a macro makes of one
    /// body record each call once.
    seen: HashSet<(String, String, u32, u32)>,
    graph: CallGraphData,
}

impl<'a> EdgeWalk<'a> {
    /// Walk `hir`, whose calls belong to `current`. A lambda the source names
    /// takes its own name, and an anonymous one keeps its enclosing function's.
    fn walk(&mut self, hir: &Hir, current: Option<&'a str>) {
        match &hir.kind {
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, value) in bindings {
                    self.walk_value(*binding, value, current);
                }
                self.walk(body, current);
            }
            HirKind::Define { binding, value } => self.walk_value(*binding, value, current),
            HirKind::Call { func, is_tail, .. } => {
                if let Some(caller) = current {
                    let at = CallAt {
                        line: hir.span.line,
                        col: hir.span.col,
                        is_tail: *is_tail,
                    };
                    self.record(caller, func, at);
                }
                hir.for_each_child(|child| self.walk(child, current));
            }
            _ => hir.for_each_child(|child| self.walk(child, current)),
        }
    }

    fn walk_value(&mut self, binding: Binding, value: &Hir, current: Option<&'a str>) {
        let named = defined_name(binding, value, self.arena, self.symbols);
        self.walk(value, named.or(current));
    }

    /// Record the call `func` stands in, when the source spelled `func`.
    fn record(&mut self, caller: &str, func: &Hir, at: CallAt) {
        if !self.written.contains(&func.id) {
            return;
        }
        let Some(callee) = spelled(func, self.arena, self.symbols) else {
            return;
        };
        let key = (caller.to_string(), callee.clone(), at.line, at.col);
        if !self.seen.insert(key) {
            return;
        }
        let signal = match &func.kind {
            HirKind::Var(binding) => self.signals.get(binding).copied(),
            _ => None,
        };
        self.graph
            .reverse
            .entry(callee.clone())
            .or_default()
            .push(CallSite {
                caller: caller.to_string(),
                at,
            });
        self.graph
            .edges
            .entry(caller.to_string())
            .or_default()
            .push(CallEdge { callee, at, signal });
    }
}

/// The call graph of the calls the source spells. `written` holds the nodes
/// built for the symbols the source spells, and `fixed` the signal of each
/// binding the analysis started with.
pub(crate) fn build_call_graph(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    signal_map: &HashMap<String, Signal>,
    fixed: &HashMap<Binding, Signal>,
    written: &HashSet<HirId>,
) -> CallGraphData {
    let mut signals = fixed.clone();
    each_definition(hir, &mut |binding, value| {
        if let Some(lambda) = value.kind.as_lambda() {
            if !arena.get(binding).is_mutated {
                signals.insert(binding, lambda.inferred_signals);
            }
        }
    });
    let mut walk = EdgeWalk {
        arena,
        symbols,
        written,
        signals,
        seen: HashSet::new(),
        graph: CallGraphData::default(),
    };
    walk.walk(hir, None);
    let mut graph = walk.graph;

    // A self-call makes a function neither called by another nor a caller of
    // another, so both lists skip it.
    let others = |caller: &String| -> Vec<String> {
        graph.edges.get(caller).map_or_else(Vec::new, |edges| {
            edges
                .iter()
                .filter(|e| e.callee != *caller)
                .map(|e| e.callee.clone())
                .collect()
        })
    };
    let defined: BTreeSet<&String> = signal_map.keys().collect();
    let called: HashSet<String> = graph.edges.keys().flat_map(others).collect();
    let roots = defined
        .iter()
        .filter(|name| !called.contains(name.as_str()))
        .map(|name| name.to_string())
        .collect();
    let leaves = defined
        .iter()
        .filter(|name| !others(name).iter().any(|c| signal_map.contains_key(c)))
        .map(|name| name.to_string())
        .collect();
    graph.roots = roots;
    graph.leaves = leaves;
    graph
}
