// audited: 2026-10-21
//! The arg-return summary: for each inlinable lambda binding, the fixed-parameter indices that reach its tail.
//!
//! docs/impl/escape.md

use rustc_hash::FxHashMap;

use super::atom::Atom;
use super::sources::tail_sources;
use super::TailCtx;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};

/// The arg-return summary, keyed by lambda binding.
///
/// A binding is inlinable when a `Let` or `Letrec` binds it to a `Lambda`,
/// which is where the region solver fills `binding_lambda`. A top-level
/// `Define` never qualifies. `for_each_tail`'s `Call` arm applies the
/// immutable, unmutated guard where it reads the summary.
///
/// The summary is computed to a fixpoint: `(fn (z) (id z))` returns its
/// argument only once `id`'s own entry says `id` returns argument 0. A
/// returned index is never withdrawn, so the loop ends.
pub(in crate::hir::escape) fn compute_arg_return(
    hir: &Hir,
    arena: &BindingArena,
) -> FxHashMap<Binding, Vec<usize>> {
    // Recompute every inlinable lambda's returned-parameter set against the
    // current summary. Each body is processed during the visit, so nothing
    // borrows past it.
    fn refine(ctx: &TailCtx, h: &Hir, out: &mut Vec<(Binding, Vec<usize>)>) {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &h.kind {
            for (b, init) in bindings {
                if let HirKind::Lambda(lambda) = &init.kind {
                    let mut srcs = Vec::new();
                    tail_sources(ctx, &lambda.body, &mut srcs);
                    let returned: Vec<usize> = lambda
                        .params
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| srcs.contains(&Atom::Binding(**p)))
                        .map(|(i, _)| i)
                        .collect();
                    out.push((*b, returned));
                }
            }
        }
        h.for_each_child(|c| refine(ctx, c, out));
    }

    let mut summary: FxHashMap<Binding, Vec<usize>> = FxHashMap::default();
    loop {
        // Recompute against a frozen snapshot, then apply, so no lambda reads a
        // half-updated summary within one iteration.
        let mut updates: Vec<(Binding, Vec<usize>)> = Vec::new();
        {
            let ctx = TailCtx {
                arena,
                arg_return: &summary,
            };
            refine(&ctx, hir, &mut updates);
        }
        let mut changed = false;
        for (b, returned) in updates {
            if summary.get(&b).map(Vec::as_slice) != Some(returned.as_slice()) {
                summary.insert(b, returned);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    summary
}
