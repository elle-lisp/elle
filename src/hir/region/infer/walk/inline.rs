// audited: 2026-10-28
//! Callee inlining for the region walk: re-walk a known callee's body so its intrinsics record edges at the call site.
//!
//! docs/impl/region/mechanism.md
//!
//! The inline temporarily binds a Var-callee Lambda's params to the caller's arg
//! regions and re-walks its body. What crosses back is only what the walk
//! RECORDED, plus the one summary fact [`Inlined`] carries. The regions the walk
//! yields name the callee's activation, so the caller names the call's own
//! region for the result, and the callee's own naming is put back
//! ([`CalleeNaming`]).

use super::*;
use std::collections::HashMap;

/// The callee's own naming as the walk held it before an inline: what each
/// binding the callee introduces names, the scope each sits in, and the region
/// of each scope node. The re-walk names all three against one call site, and
/// the callee compiles to one body for every call, so the inline puts them back.
///
/// Saved once, before any write. A collector parameter (`&`, `&keys`,
/// `&named`) is one binding in both `params` and `rest_param`. A save per write
/// would record the param write's value as the rest write's "before", and the
/// collector would end naming no region (pinned by
/// `tests/impl/region-inline-rest-param-leak.lisp`).
struct CalleeNaming {
    bindings: Vec<(Binding, Option<Vec<Region>>, Option<Region>)>,
    scopes: Vec<(HirId, Option<Region>)>,
}

impl CalleeNaming {
    fn save(walk: &RegionInference, lambda: &Hir) -> Self {
        fn visit(h: &Hir, walk: &RegionInference, out: &mut Vec<(HirId, Option<Region>)>) {
            out.push((h.id, walk.scope_region.get(&h.id).copied()));
            h.for_each_child(|c| visit(c, walk, out));
        }
        let mut introduced = Vec::new();
        lambda.introduced_bindings(&mut introduced);
        let mut seen = rustc_hash::FxHashSet::default();
        let bindings = introduced
            .into_iter()
            .filter(|b| seen.insert(*b))
            .map(|b| {
                let regions = walk.binding_regions.get(&b).cloned();
                (b, regions, walk.binding_region.get(&b).copied())
            })
            .collect();
        let mut scopes = Vec::new();
        visit(lambda, walk, &mut scopes);
        CalleeNaming { bindings, scopes }
    }

    fn restore(self, walk: &mut RegionInference) {
        for (b, regions, scope) in self.bindings {
            put_back(&mut walk.binding_regions, b, regions);
            put_back(&mut walk.binding_region, b, scope);
        }
        for (id, scope) in self.scopes {
            put_back(&mut walk.scope_region, id, scope);
        }
    }
}

/// Make `map[key]` what it was: `prior`, or absent.
fn put_back<K: std::hash::Hash + Eq, V>(map: &mut HashMap<K, V>, key: K, prior: Option<V>) {
    match prior {
        Some(v) => {
            map.insert(key, v);
        }
        None => {
            map.remove(&key);
        }
    }
}

/// What an inlined callee's body walk tells its CALL SITE.
///
/// Not the body's regions: those name the callee's activation and are remapped
/// to fresh physical regions per call. Handing them to the caller would make it
/// a nominal holder of a region it never allocates (see the module doc). What
/// does cross back is the one fact the caller cannot read off the callee's
/// declaration: whether the call yields a heap value at all. That is the
/// question `call_returns_immediate` answers for a native, asked of a body this
/// compilation can see. It decides whether the call names its own region or
/// names none.
pub(super) enum Inlined {
    /// No resolvable lambda body here — fall back to opaque-call handling.
    No,
    /// Body walked; its result is an immediate, so the call names no region.
    Immediate,
    /// Body walked; its result is a heap value, named by the call's own region.
    Heap,
}

impl RegionInference {
    /// Try to inline a Call's callee Lambda body for region analysis.
    ///
    /// When the callee is a Var whose binding has a known Lambda init
    /// (recorded in `binding_lambda`), temporarily bind the Lambda's
    /// params to the caller's arg source regions and walk the body.
    /// This lets the walk see intrinsics inside the body (for example a
    /// `%array-push`) and emit the corresponding cross-region edges at the
    /// call site.
    ///
    /// Returns [`Inlined`]: whether the body was walked, and — when it was — the
    /// one summary fact that crosses back to the call site. The body's own regions
    /// do not (see [`Inlined`]).
    pub(super) fn try_inline_call(
        &mut self,
        func: &Hir,
        arg_regions: &[Vec<Region>],
        _call_id: HirId,
    ) -> Inlined {
        // Only inline Var callees.
        let binding = match &func.kind {
            HirKind::Var(b) => *b,
            _ => return Inlined::No,
        };
        // Must be immutable and have a known Lambda body.
        let bi = self.arena().get(binding);
        if !bi.is_immutable || bi.is_mutated {
            return Inlined::No;
        }
        let Some(&lambda_ptr) = self.binding_lambda.get(&binding) else {
            return Inlined::No;
        };
        // Bound the inline nesting at four levels.
        if self.inline_depth >= 4 {
            return Inlined::No;
        }
        // A call to a lambda whose body the walk is already inside starts a new
        // activation of that body. The re-walk would name the new activation's
        // allocations by this walk's structural regions, so an edge from a
        // caller region into one of them reads as containment between two
        // regions of ONE activation (docs/impl/region/mechanism.md).
        if self.walking_lambdas.contains(&lambda_ptr) {
            return Inlined::No;
        }
        // SAFETY: lambda_ptr points into the HIR tree which outlives
        // the RegionInference (both live for the analyze_regions_with call).
        let lambda = unsafe { &*lambda_ptr };
        let (params, rest_param, body) = match &lambda.kind {
            HirKind::Lambda(lambda) => (&lambda.params, &lambda.rest_param, &lambda.body),
            _ => return Inlined::No,
        };
        let naming = CalleeNaming::save(self, lambda);
        // Bind params to the caller's arg regions.
        for (i, p) in params.iter().enumerate() {
            let regions = arg_regions.get(i).cloned().unwrap_or_default();
            self.binding_regions.insert(*p, regions);
            self.binding_region.insert(*p, self.current_region);
        }
        // The rest write lands last on a collector binding, and must: the
        // collected value is built by the callee's own calling convention, so it
        // belongs to no caller region.
        if let Some(rp) = rest_param {
            self.binding_regions.insert(*rp, Vec::new());
            self.binding_region.insert(*rp, self.current_region);
        }
        // Mark the caller's arg regions live for this inline so a `Return`
        // reached in the body does not extend a caller region's `decref_point`
        // to a callee node (see `inline_bound_regions`). Track only those we
        // newly add — a region an OUTER inline already marked must stay marked
        // when this one exits.
        let mut newly_bound: Vec<Region> = Vec::new();
        for regions in arg_regions {
            for &r in regions {
                if self.inline_bound_regions.insert(r) {
                    newly_bound.push(r);
                }
            }
        }
        // The body being walked is a LAMBDA's body, so the walk is inside that
        // lambda's activation. Enter it at the callee's lambda depth, not the
        // caller's. Every `in_lambda()` reader asks "is this node inside a lambda
        // body", a structural fact of the node and not of who reached it. The
        // readers include the reassign gate's module-scope-vs-fn-local split
        // (docs/impl/region/bindings.md) and the `Begin`/`Let`/`Letrec`
        // compiled-capture-cell mints, which follow the lowerer's own
        // `in_lambda`. Bypassing the `Lambda` arm's own bump would answer each of them
        // with the call site's nesting.
        self.in_lambda_depth += 1;
        self.inline_depth += 1;
        // The walk is run for its RECORDING side effects — the edges, sites and
        // classifications the body's intrinsics contribute at this call site. Of its
        // result regions only the EMPTINESS crosses back (see [`Inlined`]); the
        // regions themselves are the callee's own and are discarded.
        self.walking_lambdas.push(lambda_ptr);
        let yields_heap = !self.walk(body).is_empty();
        self.walking_lambdas.pop();
        self.inline_depth -= 1;
        self.in_lambda_depth -= 1;
        for r in newly_bound {
            self.inline_bound_regions.remove(&r);
        }
        naming.restore(self);
        if yields_heap {
            Inlined::Heap
        } else {
            Inlined::Immediate
        }
    }
}
