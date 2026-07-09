//! The two structural candidate sweeps that seed the call and fiber faces:
//! producer bindings (a sole-Lambda init) and fiber-body bindings (a
//! `fiber/new` site whose arg-0 resolves to a Lambda), each collected in walk
//! order before the per-candidate gates run.
//!
//! The collected `*const Hir` pointers index nodes of the HIR tree, which
//! outlives the analysis — the established idiom for keeping HIR references
//! across a walk (see `UseIndex`).

use super::*;

/// Producer candidates: a binding with a sole-Lambda init whose init node is
/// exactly that lambda (`(Binding, lambda ptr)`), in walk order.
pub(super) fn collect_producers(
    h: &Hir,
    arena: &BindingArena,
    ix: &UseIndex,
    out: &mut Vec<(Binding, *const Hir)>,
) {
    match &h.kind {
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                if let Some(l) = ix.sole_lambda_init(*b, arena) {
                    if std::ptr::eq(l, unwrap_cell(init)) {
                        out.push((*b, l as *const Hir));
                    }
                }
                collect_producers(init, arena, ix, out);
            }
            collect_producers(body, arena, ix, out);
        }
        HirKind::Define { binding, value } => {
            if let Some(l) = ix.sole_lambda_init(*binding, arena) {
                if std::ptr::eq(l, unwrap_cell(value)) {
                    out.push((*binding, l as *const Hir));
                }
            }
            collect_producers(value, arena, ix, out);
        }
        _ => h.for_each_child(|c| collect_producers(c, arena, ix, out)),
    }
}

/// `(fiber binding, fiber/new site, body lambda, the body's own binding when it
/// came through a Var)`, in walk order.
pub(super) type FiberCand = (Binding, HirId, *const Hir, Option<Binding>);

/// Fiber-body candidates: each `fiber/new` site whose arg-0 resolves (through
/// the ANF/cell wrappers, directly or via a sole-Lambda binding) to a Lambda.
pub(super) fn collect_fibers(
    h: &Hir,
    ix: &UseIndex,
    arena: &BindingArena,
    cc: &CallClassification,
    out: &mut Vec<FiberCand>,
) {
    if let HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } = &h.kind {
        for (b, init) in bindings {
            let call = anf_tail(init);
            if let HirKind::Call { func, args, .. } = &call.kind {
                let sym = callee_symbol(func, arena);
                if sym.is_some() && sym == cc.fiber_new {
                    let body_lambda =
                        args.first()
                            .and_then(|a| match &unwrap_cell(anf_tail(&a.expr)).kind {
                                HirKind::Lambda { .. } => {
                                    Some((unwrap_cell(anf_tail(&a.expr)) as *const Hir, None))
                                }
                                HirKind::Var(fb) => ix
                                    .sole_lambda_init(*fb, arena)
                                    .map(|l| (l as *const Hir, Some(*fb))),
                                _ => None,
                            });
                    if let Some((l, fb)) = body_lambda {
                        out.push((*b, call.id, l, fb));
                    }
                }
            }
            collect_fibers(init, ix, arena, cc, out);
        }
        collect_fibers(body, ix, arena, cc, out);
        return;
    }
    h.for_each_child(|c| collect_fibers(c, ix, arena, cc, out));
}
