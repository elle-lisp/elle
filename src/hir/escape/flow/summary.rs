//! The interprocedural arg-return summary: per inlinable lambda binding, which
//! fixed-param indices flow to its tail, computed to a fixpoint. This is the
//! compile-time analogue of the solver's `try_inline_call`.

use rustc_hash::FxHashMap;

use super::atom::Atom;
use super::sources::tail_sources;
use super::TailCtx;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};

/// The arg-return summary: per inlinable lambda binding, which **fixed-param
/// indices** flow to its tail under interprocedural transparency. This is the
/// compile-time analogue of the solver's `try_inline_call` (`regions/walk.rs`),
/// which re-walks an inlinable callee body with params bound to the caller's arg
/// regions and returns the body's tail regions — for an arg-returning callee that
/// region is the arg's, so the arg escapes the caller's tail.
///
/// "Inlinable" mirrors the solver's `binding_lambda` exactly: a binding bound to a
/// `Lambda` by a `Let` or `Letrec` (never a top-level `Define` — the solver does
/// not populate `binding_lambda` there, so a `def`-bound callee is opaque and an
/// arg returned through it does NOT reach the caller's tail). The immutable/
/// unmutated guard is applied at the *consumption* site (`tail_sources`'s `Call`
/// arm), matching `try_inline_call`'s call-site check.
///
/// Computed to a fixpoint because the property is transitive: `(fn (z) (id z))`
/// returns its arg only once `id`'s own summary says `id` returns arg 0. The
/// fixpoint is monotonic (a returned index is never withdrawn), bounded by the
/// total param count, so it converges. The bounded re-walk in `try_inline_call`
/// stops at inline depth 4; this whole-program fixpoint can in principle propagate
/// through a deeper arg-return chain than the solver inlines, which would
/// *over-approximate* (mark a true escape the solver misses) — sound, and not
/// observed in the corpus; if a real-corpus golden ever surfaces it, bound the
/// propagation depth to match.
pub(in crate::hir::escape) fn compute_arg_return(
    hir: &Hir,
    arena: &BindingArena,
) -> FxHashMap<Binding, Vec<usize>> {
    // Recompute every inlinable lambda's returned-param set against the current
    // summary; visit Let/Letrec-bound lambdas anywhere in the tree, mirroring the
    // solver's `binding_lambda` population (never a top-level `Define`). Bodies are
    // processed inline during the walk, so nothing borrows past the visit.
    fn refine(ctx: &TailCtx, h: &Hir, out: &mut Vec<(Binding, Vec<usize>)>) {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &h.kind {
            for (b, init) in bindings {
                if let HirKind::Lambda { params, body, .. } = &init.kind {
                    let mut srcs = Vec::new();
                    tail_sources(ctx, body, &mut srcs);
                    let returned: Vec<usize> = params
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
        // Recompute against a frozen snapshot, then apply — so no lambda observes
        // a half-updated summary within one iteration. A Binding id reused across
        // sibling file-letrec re-defs appears more than once; source order means
        // the last application wins, matching the solver's `binding_lambda.insert`.
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
