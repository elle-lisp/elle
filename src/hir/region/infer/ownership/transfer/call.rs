// audited: 2026-10-21
//! The call face: a producer lambda bound once and used only as a callee, whose every call site passes the consumer gate.
//!
//! docs/impl/region/owner.md

use super::*;

/// Admit each producer binding in structural walk order: one Lambda init, used
/// ONLY as a callee, every call site consumer-admitted.
pub(super) fn admit(
    cut: &Cut,
    hir: &Hir,
    arena: &BindingArena,
    taken_roots: &mut FxHashSet<Region>,
    admitted: &mut Vec<(Summary, Vec<Region>)>,
) {
    let mut candidates: Vec<(Binding, *const Hir)> = Vec::new();
    collect_producers(hir, arena, &cut.ix, &mut candidates);
    for (f, l) in candidates {
        // SAFETY: `l` points into the HIR tree, which outlives this call.
        let l = unsafe { &*l };
        let HirKind::Lambda(lambda) = &l.kind else {
            continue;
        };
        // Every occurrence of `f` must be a callee position — any other use (an
        // alias, a HOF hand-off, a store) is an unknown consumer.
        let Some(uses) = cut.ix.uses.get(&f) else {
            continue;
        };
        let sites: Vec<HirId> = uses
            .iter()
            .filter_map(|u| match u {
                UseForm::Callee(site) => Some(*site),
                _ => None,
            })
            .collect();
        if sites.is_empty() || sites.len() != uses.len() {
            continue;
        }
        let Some(summary) = cut.summarize(l, &lambda.body) else {
            continue;
        };
        if taken_roots.contains(&summary.root) {
            continue;
        }
        let producer = cut.interval(l);
        let Some(results) = sites
            .iter()
            .map(|&s| cut.consumer_result(s, summary.root, producer))
            .collect::<Option<Vec<Region>>>()
        else {
            continue;
        };
        taken_roots.insert(summary.root);
        admitted.push((summary, results));
    }
}
