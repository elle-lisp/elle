use super::super::*;
use super::*;

/// A binding initializer with any capture-cell wrapper peeled off: a
/// self-recursive or captured lambda arrives as `MakeCell { Lambda }`, and
/// the lambda inside is what inference cares about.
pub(crate) fn unwrap_make_cell(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::MakeCell { value } => value,
        _ => h,
    }
}

/// Bindings written by `Assign`/`SetCell` anywhere in the tree. A parameter
/// in this set never receives call-site proofs — its value after the first
/// write is invisible to the per-pass recomputation.
pub(crate) fn collect_mutated_bindings(hir: &Hir) -> std::collections::HashSet<Binding> {
    let mut out = std::collections::HashSet::new();
    fn go(h: &Hir, out: &mut std::collections::HashSet<Binding>) {
        match &h.kind {
            HirKind::Assign { target, .. } => {
                out.insert(*target);
            }
            HirKind::SetCell { cell, .. } => {
                if let HirKind::Var(b) = &cell.kind {
                    out.insert(*b);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| go(c, out));
    }
    go(hir, &mut out);
    out
}

/// Collect every binding read in VALUE position — anywhere except as the
/// direct callee of a `Call`. A lambda binding with even one value-position
/// use (stored, passed as an argument, returned, exported) has callers the
/// call-site scan cannot see, so its parameter joins are not a proof
/// (`infer_node`'s forwarding consults this set).
pub(crate) fn collect_value_position_uses(hir: &Hir, out: &mut std::collections::HashSet<Binding>) {
    match &hir.kind {
        HirKind::Call { func, args, .. } => {
            // The callee itself is a callee-position use; anything nested
            // deeper in a computed callee is a value use.
            match &func.kind {
                HirKind::Var(_) => {}
                HirKind::DerefCell { cell } if matches!(cell.kind, HirKind::Var(_)) => {}
                _ => collect_value_position_uses(func, out),
            }
            for a in args {
                collect_value_position_uses(&a.expr, out);
            }
        }
        HirKind::Var(b) => {
            out.insert(*b);
        }
        HirKind::DerefCell { cell } => {
            if let HirKind::Var(b) = &cell.kind {
                out.insert(*b);
            } else {
                collect_value_position_uses(cell, out);
            }
        }
        _ => {
            hir.for_each_child(|c| collect_value_position_uses(c, out));
        }
    }
}

/// Collect which bindings are lambda definitions and what their params are,
/// plus the params covered by a `(numeric!)` declaration (their caller-join is
/// floored at Number — the declared contract holds whatever callers pass).
pub(crate) fn collect_lambda_info(
    hir: &Hir,
    _arena: &BindingArena,
    lambda_params: &mut HashMap<Binding, Vec<Binding>>,
    declared_numeric: &mut std::collections::HashSet<Binding>,
) {
    let record = |binding: &Binding,
                  value: &Hir,
                  lambda_params: &mut HashMap<Binding, Vec<Binding>>,
                  declared_numeric: &mut std::collections::HashSet<Binding>| {
        if let HirKind::Lambda {
            params,
            assert_numeric,
            ..
        } = &unwrap_make_cell(value).kind
        {
            lambda_params.insert(*binding, params.clone());
            if *assert_numeric {
                declared_numeric.extend(params.iter().copied());
            }
        }
    };
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                record(binding, value, lambda_params, declared_numeric);
                collect_lambda_info(value, _arena, lambda_params, declared_numeric);
            }
            collect_lambda_info(body, _arena, lambda_params, declared_numeric);
        }
        HirKind::Define { binding, value } => {
            record(binding, value, lambda_params, declared_numeric);
            collect_lambda_info(value, _arena, lambda_params, declared_numeric);
        }
        _ => {
            hir.for_each_child(|child| {
                collect_lambda_info(child, _arena, lambda_params, declared_numeric)
            });
        }
    }
}

/// Map each immutable, unmutated binding whose initializer is `(type-of a)` to
/// its subject `a` — provided `a` is itself unmutated, so the `type-of` measured
/// at the binding still describes `a` at the later match scrutinee. Feeds
/// `typeof_subject_binding` so the `(let [ta (type-of a)] (match ta …))` idiom
/// narrows and prunes exactly like the inline `(match (type-of a) …)` form. Both
/// conditions are soundness requirements, enforced conservatively: the alias must
/// be a stable single value (a rebindable alias could hold a different value at
/// the match — the same `is_immutable && !is_mutated` gate `prune.rs`'s
/// `collect_inits` uses), and the subject must be stable (a reassigned subject
/// could hold a different type than the arm proves). The subject gate is
/// belt-and-suspenders — a mutated subject is cell-held and its `DerefCell` reads
/// are not narrowed by the binding-type override anyway — but it keeps this
/// resolution self-evidently sound without depending on that distant fact.
pub(crate) fn collect_typeof_aliases(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    out: &mut HashMap<Binding, Binding>,
) {
    let record = |b: Binding, init: &Hir, out: &mut HashMap<Binding, Binding>| {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        let inner = unwrap_anf_let(unwrap_make_cell(init));
        if let Some(subj) = typeof_call_subject(inner, arena, symbol_names) {
            if !arena.get(subj).is_mutated {
                out.insert(b, subj);
            }
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                record(*b, init, out);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, out),
        _ => {}
    }
    hir.for_each_child(|c| collect_typeof_aliases(c, arena, symbol_names, out));
}
