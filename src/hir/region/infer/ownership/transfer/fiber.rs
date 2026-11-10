// audited: 2026-10-21
//! The fiber face: a fiber whose body's terminal value is a summarized subtree, handed back by each completing resume.
//!
//! docs/impl/region/owner.md
//!
//! Every other resume outcome is a fresh error struct or an immediate, each
//! safely adoptable. The channel's idempotence absorbs a re-delivered
//! masked-error payload, and the same-function gate bounds it to one activation.

use super::*;
use crate::hir::Lambda;

/// Admit each `fiber/new` site whose body lambda summarizes and whose every use
/// is a gated resume or an `Immediate`-native read.
pub(super) fn admit(
    cut: &Cut,
    hir: &Hir,
    arena: &BindingArena,
    call_class: &CallClassification,
    taken_roots: &mut FxHashSet<Region>,
    admitted: &mut Vec<(Summary, Vec<Region>)>,
) {
    let mut fibers: Vec<FiberCand> = Vec::new();
    collect_fibers(hir, &cut.ix, arena, call_class, &mut fibers);
    for (f2, new_site, l, body_binding) in fibers {
        // SAFETY: `l` points into the HIR tree, which outlives this call.
        let l = unsafe { &*l };
        let bi = arena.get(f2);
        if !bi.is_immutable || bi.is_mutated || cut.captured.contains(&f2) {
            continue;
        }
        // The body must be unable to deliver a non-terminal value: no yield / io /
        // debug / wait bits and not polymorphic. Errors are fine — an error
        // delivery is a fresh struct, and a restarted re-delivery of the same
        // payload lands in the same activation, where the channel's idempotence
        // absorbs it.
        let Some(Lambda {
            body,
            inferred_signals,
            ..
        }) = l.kind.as_lambda()
        else {
            continue;
        };
        let suspending = crate::signals::SIG_YIELD
            .union(crate::signals::SIG_IO)
            .union(crate::signals::SIG_DEBUG)
            .union(crate::signals::SIG_WAIT);
        if inferred_signals.propagates != 0 || inferred_signals.bits.intersects(suspending) {
            continue;
        }
        // When the body lambda came through a binding, that binding's every use
        // must be a fiber-body position — a body ALSO called directly (or handed
        // anywhere else) has un-gated consumers.
        if let Some(fb) = body_binding {
            let all_fiber_arg = cut
                .ix
                .uses
                .get(&fb)
                .is_some_and(|us| us.iter().all(|u| matches!(u, UseForm::FiberNewArg0(_))));
            if !all_fiber_arg {
                continue;
            }
        }
        // The fiber's own region: fresh, unaliased, edge-free, held only by this
        // binding.
        let Some(&rf2) = cut.info.alloc_region.get(&new_site) else {
            continue;
        };
        if cut.inputs.shared().contains(&rf2) || cut.in_any_edge(rf2) || !cut.inputs.sole_held(rf2)
        {
            continue;
        }
        if cut
            .info
            .binding_source_regions
            .get(&f2)
            .map(|v| v.as_slice())
            != Some([rf2].as_slice())
        {
            continue;
        }
        let Some(resume_sites) = resume_sites(cut, f2, new_site) else {
            continue;
        };
        let Some(summary) = cut.summarize(l, body) else {
            continue;
        };
        if taken_roots.contains(&summary.root) {
            continue;
        }
        let producer = cut.interval(l);
        let Some(results) = resume_sites
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

/// The resume sites of fiber binding `f2`, when every use of it is a resume (a
/// gated consumer site) or an `Immediate`-native read, all in the SAME function
/// body as the `fiber/new` site. Each activation then drives its own private
/// fiber, so no delivery can outlive the adopting activation.
fn resume_sites(cut: &Cut, f2: Binding, new_site: HirId) -> Option<Vec<HirId>> {
    let uses = cut.ix.uses.get(&f2)?;
    let new_encl = cut.ix.enclosing.get(&new_site).copied().unwrap_or(None);
    let mut sites: Vec<HirId> = Vec::new();
    for u in uses {
        match u {
            UseForm::ResumeArg0(site) => {
                if cut.ix.enclosing.get(site).copied().unwrap_or(None) != new_encl {
                    return None;
                }
                sites.push(*site);
            }
            UseForm::ImmediateArg => {}
            _ => return None,
        }
    }
    (!sites.is_empty()).then_some(sites)
}
