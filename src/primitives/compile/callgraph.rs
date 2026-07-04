use super::*;

pub(crate) fn build_signal_map(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
) -> HashMap<String, Signal> {
    let mut map = HashMap::new();
    collect_fn_signals(hir, arena, symbols, &mut map);
    map
}
pub(crate) fn collect_fn_signals(
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
        HirKind::Emit { value: expr, .. }
        | HirKind::Break { value: expr, .. }
        | HirKind::Return { value: expr } => {
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
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_) => {}

        HirKind::Error => {}
    }
}
pub(crate) fn build_call_graph(
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
pub(crate) fn collect_call_edges(
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
        HirKind::Emit { value: expr, .. }
        | HirKind::Break { value: expr, .. }
        | HirKind::Return { value: expr } => {
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
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_) => {}

        HirKind::Error => {}
    }
}
