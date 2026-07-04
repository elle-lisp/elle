//! Callee fixpoint pre-pass: classify letrec-bound lambdas that provably
//! return an immediate (`super` = `hir::regions`).

use super::*;

// ── Callee fixpoint pre-pass ────────────────────────────────────

/// Classify letrec-bound lambdas: does the body provably return an immediate?
///
/// Iterates to a fixpoint because function A may call function B
/// (both letrec-bound), so A's classification depends on B's.
pub(super) fn classify_letrec_callees(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &CallClassification,
) -> rustc_hash::FxHashSet<Binding> {
    use rustc_hash::FxHashSet;

    // Step 1: collect letrec-bound lambdas (binding → lambda body)
    let mut lambda_bodies: HashMap<Binding, &Hir> = HashMap::new();
    collect_letrec_lambdas(hir, &mut lambda_bodies);

    if lambda_bodies.is_empty() {
        return FxHashSet::default();
    }

    // Step 2: fixpoint iteration
    let mut immediates: FxHashSet<Binding> = FxHashSet::default();
    loop {
        let mut changed = false;
        for (&binding, body) in &lambda_bodies {
            if immediates.contains(&binding) {
                continue;
            }
            if body_returns_immediate(body, arena, call_class, &immediates) {
                immediates.insert(binding);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    immediates
}

/// Walk the HIR to find letrec-bound lambdas.
fn collect_letrec_lambdas<'a>(hir: &'a Hir, out: &mut HashMap<Binding, &'a Hir>) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                if matches!(&init.kind, HirKind::Lambda { .. }) {
                    out.insert(*b, init);
                }
                collect_letrec_lambdas(init, out);
            }
            collect_letrec_lambdas(body, out);
        }
        HirKind::Let { bindings, body } => {
            for (_, init) in bindings {
                collect_letrec_lambdas(init, out);
            }
            collect_letrec_lambdas(body, out);
        }
        HirKind::Lambda { body, .. } => {
            collect_letrec_lambdas(body, out);
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_letrec_lambdas(cond, out);
            collect_letrec_lambdas(then_branch, out);
            collect_letrec_lambdas(else_branch, out);
        }
        HirKind::Begin(exprs) => {
            for e in exprs {
                collect_letrec_lambdas(e, out);
            }
        }
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                collect_letrec_lambdas(init, out);
            }
            collect_letrec_lambdas(body, out);
        }
        HirKind::Block { body, .. } => {
            for e in body {
                collect_letrec_lambdas(e, out);
            }
        }
        HirKind::Define { value, .. } => {
            collect_letrec_lambdas(value, out);
        }
        _ => {}
    }
}

/// Does a lambda body provably return an immediate value?
///
/// Conservative: returns false for anything uncertain. For Lambda nodes,
/// checks the body (the last expression determines return type).
fn body_returns_immediate(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &CallClassification,
    user_immediates: &rustc_hash::FxHashSet<Binding>,
) -> bool {
    match &hir.kind {
        // Literals are immediate
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::Keyword(_) => true,

        // Strings/quotes allocate
        HirKind::String(_) | HirKind::Quote(_) => false,

        // Lambda: check the body to classify the function's return type
        HirKind::Lambda { body, .. } => {
            body_returns_immediate(body, arena, call_class, user_immediates)
        }

        // Non-allocating intrinsics return immediates
        HirKind::Intrinsic { op, .. } => !op.allocates(),

        // Var: conservative — could be anything
        HirKind::Var(_) => false,

        // Call: check if callee is known immediate-returning
        HirKind::Call { func, .. } => {
            if let HirKind::Var(binding) = &func.kind {
                let bi = arena.get(*binding);
                if !bi.is_immutable || bi.is_mutated {
                    return false;
                }
                let sym = bi.name;
                call_class.intrinsic_ops.contains(&sym)
                    || call_class.effects.get(&sym)
                        == Some(&crate::primitives::def::RegionEffect::Immediate)
                    || user_immediates.contains(binding)
            } else {
                false
            }
        }

        // Begin: last expression's type
        HirKind::Begin(exprs) => exprs
            .last()
            .map(|e| body_returns_immediate(e, arena, call_class, user_immediates))
            .unwrap_or(true), // empty begin → nil

        // If: both branches must be immediate
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            body_returns_immediate(then_branch, arena, call_class, user_immediates)
                && body_returns_immediate(else_branch, arena, call_class, user_immediates)
        }

        // Let/Letrec: body determines result
        HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
            body_returns_immediate(body, arena, call_class, user_immediates)
        }

        // Loop: body determines result (the non-recur path)
        HirKind::Loop { body, .. } => {
            body_returns_immediate(body, arena, call_class, user_immediates)
        }

        // Cond: all branches + else
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            clauses
                .iter()
                .all(|(_, b)| body_returns_immediate(b, arena, call_class, user_immediates))
                && else_branch
                    .as_ref()
                    .map(|e| body_returns_immediate(e, arena, call_class, user_immediates))
                    .unwrap_or(true)
        }

        // Match: all arms
        HirKind::Match { arms, .. } => arms
            .iter()
            .all(|(_, _, b)| body_returns_immediate(b, arena, call_class, user_immediates)),

        // And/Or: all branches
        HirKind::And(exprs) | HirKind::Or(exprs) => exprs
            .iter()
            .all(|e| body_returns_immediate(e, arena, call_class, user_immediates)),

        // Everything else: conservative
        _ => false,
    }
}
