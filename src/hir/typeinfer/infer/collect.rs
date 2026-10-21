// audited: 2026-10-21
//! The pre-passes: the program facts one walk of the tree yields, which every later pass reads.
//!
//! docs/impl/typeinfer.md

use super::*;
use crate::hir::{Lambda, VarargKind};

/// A binding initializer with any capture-cell wrapper peeled off: a
/// self-recursive or captured lambda arrives as `MakeCell { Lambda }`, and
/// the lambda inside is what inference cares about.
pub(crate) fn unwrap_make_cell(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::MakeCell { value } => value,
        _ => h,
    }
}

/// How many nodes the tree holds: the size the node-type table grows to.
pub(crate) fn count_nodes(hir: &Hir) -> usize {
    let mut n = 1;
    hir.for_each_child(|c| n += count_nodes(c));
    n
}

/// Bindings written by `Assign`/`SetCell` anywhere in the tree. A parameter
/// in this set never receives call-site proofs — its value after the first
/// write is invisible to the per-pass recomputation.
pub(crate) fn collect_mutated_bindings(hir: &Hir) -> rustc_hash::FxHashSet<Binding> {
    let mut out = rustc_hash::FxHashSet::default();
    fn go(h: &Hir, out: &mut rustc_hash::FxHashSet<Binding>) {
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
/// call-site scan cannot see, so its parameter joins are not a proof.
/// `infer_call` consults this set before it forwards operand types.
pub(crate) fn collect_value_position_uses(hir: &Hir, out: &mut rustc_hash::FxHashSet<Binding>) {
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

/// A lambda binding's parameters, split the way a call's operands reach them.
pub(crate) struct LambdaSig {
    /// Every parameter, the rest parameter included.
    pub(crate) all: Vec<Binding>,
    /// The fixed parameters, in order: the one operand in each position
    /// reaches each, and an `&opt` one a call gives no operand is nil.
    pub(crate) fixed: Vec<Binding>,
    /// The rest parameter, which receives the list of the extra operands when
    /// it collects a list, and a struct otherwise.
    pub(crate) rest: Option<(Binding, bool)>,
}

/// Collect which bindings are lambda definitions and what their params are. A
/// `(numeric!)` declaration is NOT collected here — it lives on the parameter
/// bindings themselves (`BindingInner::declared_numeric`, applied through
/// `declared_floor`), so it is readable from any binding without a map.
pub(crate) fn collect_lambda_info(
    hir: &Hir,
    _arena: &BindingArena,
    lambda_params: &mut rustc_hash::FxHashMap<Binding, LambdaSig>,
) {
    let record = |binding: &Binding,
                  value: &Hir,
                  lambda_params: &mut rustc_hash::FxHashMap<Binding, LambdaSig>| {
        if let Some(Lambda {
            params,
            rest_param,
            vararg_kind,
            ..
        }) = unwrap_make_cell(value).kind.as_lambda()
        {
            let collects_list = *vararg_kind == VarargKind::List;
            lambda_params.insert(
                *binding,
                LambdaSig {
                    all: params
                        .iter()
                        .copied()
                        .chain(rest_param.iter().copied().filter(|r| !params.contains(r)))
                        .collect(),
                    fixed: params
                        .iter()
                        .copied()
                        .filter(|p| Some(*p) != *rest_param)
                        .collect(),
                    rest: rest_param.map(|r| (r, collects_list)),
                },
            );
        }
    };
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                record(binding, value, lambda_params);
                collect_lambda_info(value, _arena, lambda_params);
            }
            collect_lambda_info(body, _arena, lambda_params);
        }
        HirKind::Define { binding, value } => {
            record(binding, value, lambda_params);
            collect_lambda_info(value, _arena, lambda_params);
        }
        _ => {
            hir.for_each_child(|child| collect_lambda_info(child, _arena, lambda_params));
        }
    }
}

/// Map each immutable, unmutated binding whose initializer is `(type-of a)` to
/// its subject `a`, provided `a` is itself unmutated. `typeof_subject_binding`
/// reads the map, so the `(let [ta (type-of a)] (match ta …))` idiom narrows
/// and prunes as the inline `(match (type-of a) …)` form does.
///
/// Both gates keep the narrowing sound. A rebindable alias could hold a
/// different value at the match, which is the gate `prune.rs`'s
/// `collect_inits` applies too. A reassigned subject could hold a type other
/// than the one the arm proves.
pub(crate) fn collect_typeof_aliases(
    hir: &Hir,
    arena: &BindingArena,
    out: &mut HashMap<Binding, Binding>,
) {
    let record = |b: Binding, init: &Hir, out: &mut HashMap<Binding, Binding>| {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        let inner = unwrap_anf_let(unwrap_make_cell(init));
        if let Some(subj) = typeof_call_subject(inner, arena) {
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
    hir.for_each_child(|c| collect_typeof_aliases(c, arena, out));
}
