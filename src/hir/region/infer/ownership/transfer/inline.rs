// audited: 2026-10-06
//! The inline face: a binding's initializer that builds the cycle and ends in its root, with no call in between.
//!
//! docs/impl/region/owner.md
//! docs/impl/graft.md
//!
//! A grafted producer leaves exactly this shape in its caller. The initializer
//! is the producer, and every binding bound outside it that holds the root is a
//! consumer. The consumer release that turns into the adopt is the root's own,
//! so the root must take the value route: a `Fresh` call result, released from
//! the slot of the binder that allocated it.

use super::*;

/// Admit each binding initializer, in structural walk order, whose returned
/// subtree summarizes and whose outside holders are all discard-shaped.
pub(super) fn admit(
    cut: &Cut,
    hir: &Hir,
    taken_roots: &mut FxHashSet<Region>,
    admitted: &mut Vec<(Summary, Vec<Region>)>,
) {
    let mut inits: Vec<*const Hir> = Vec::new();
    collect_inits(hir, &mut inits);
    for init in inits {
        // SAFETY: `init` points into the HIR tree, which outlives this call.
        let init = unsafe { &*init };
        let Some(summary) = cut.summarize(init, init) else {
            continue;
        };
        let root = summary.root;
        if taken_roots.contains(&root) || !consumers_pass(cut, init, root) {
            continue;
        }
        taken_roots.insert(root);
        admitted.push((summary, vec![root]));
    }
}

/// The inline consumer gate. The root crosses no frontier, and the lowerer
/// releases it by value. It has a holder the producer does not bind, so it is
/// not sole-held: a sole-held root is the region-rooted cuts' own, and this gate
/// keeps the faces disjoint from them. Every such holder passes
/// [`Cut::reads_pass`].
fn consumers_pass(cut: &Cut, init: &Hir, root: Region) -> bool {
    let info = cut.info;
    if cut.inputs.shared().contains(&root)
        || !info.call_result_regions.contains(&root)
        || cut.inputs.sole_held(root)
    {
        return false;
    }
    let producer = cut.interval(init);
    let mut consumers = 0usize;
    for (&b, regions) in &info.binding_source_regions {
        if !regions.contains(&root) || cut.bound_inside(b, producer) {
            continue;
        }
        if !cut.reads_pass(b) {
            return false;
        }
        consumers += 1;
    }
    consumers > 0
}

/// Every non-lambda initializer of a `let`, `letrec` or `define` binding, in
/// structural walk order. A lambda initializer is the call face's.
fn collect_inits(h: &Hir, out: &mut Vec<*const Hir>) {
    let mut record = |init: &Hir| {
        if !matches!(unwrap_cell(init).kind, HirKind::Lambda { .. }) {
            out.push(init as *const Hir);
        }
    };
    match &h.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (_, init) in bindings {
                record(init);
            }
        }
        HirKind::Define { value, .. } => record(value),
        _ => {}
    }
    h.for_each_child(|c| collect_inits(c, out));
}
