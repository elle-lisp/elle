// audited: 2026-10-21
//! Escape analysis over the canonical (functionalized + ANF) HIR: the one authority on
//! whether a value outlives its activation.
//!
//! docs/impl/escape.md
//!
//! The design document owns the four facets, every consumer, the two decisions
//! that read structural capture instead, and the precision points. This file
//! holds `EscapeInfo` and `analyze_escape`, which seeds the facets through
//! `flow` and propagates them to a fixpoint.

use rustc_hash::{FxHashMap, FxHashSet};

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirId};

mod flow;
use flow::{
    collect_container_contents, collect_flow, compute_arg_return, record_frontier_sites,
    return_atoms, Atom, Flow, TailCtx,
};

/// Authoritative escape facts for a compilation unit.
///
/// Membership encodes "escapes"; absence is the default ("does not escape"), so
/// an empty `EscapeInfo` (`empty()`) reports nothing as escaping. The sets are
/// populated by `analyze_escape`; consumers query through the methods, never the
/// fields, so the internal representation can change without touching call sites.
#[derive(Debug, Default, Clone)]
pub struct EscapeInfo {
    /// Bindings whose value escapes its defining activation (any facet —
    /// return, store, capture, or fiber).
    binding_escapes: FxHashSet<Binding>,
    /// Lambda nodes (keyed by their `HirId`) whose closure escapes its
    /// definition.
    lambda_escapes: FxHashSet<HirId>,
    /// Bindings whose value escapes specifically via the **return facet** — it
    /// flows to a function's tail/return, an ownership transfer to the caller.
    /// A strict sub-question of `binding_escapes`: it excludes store, capture,
    /// and fiber escapes, and (unlike the full set) does not propagate through
    /// capture edges.
    ///
    /// The reassign 1-slot-container gate's "not-returned" check reads this, per
    /// binding. The gate refuses a *returned* value, which would give one
    /// transferred reference two static owners. It keeps a value that is only
    /// stored or captured, because those holders are counted at run time.
    ///
    /// Together with `return_frontier_sites` it is the return half of the
    /// ownership Shared seed, projected to regions by `region::infer::escape`. A
    /// cell that only points at a region some function returns does not flow to a
    /// tail, so it is not in this set.
    binding_returns: FxHashSet<Binding>,
    /// Bindings whose value escapes by a **containment** facet — stored by an
    /// uncounted store, captured by a closure that itself escapes, or held by a
    /// `%pair` a contained binding holds. A pair's own seeds stay out, since a pair
    /// counts its hold on its operands, and so do the fiber seeds.
    ///
    /// The split exists because the two groups answer the *holder* question
    /// differently. A containment escape hands the value to a holder the frame
    /// cannot see and, for a declared native store, cannot count. A fiber crossing
    /// hands it to a seam that counts its own reference: the park's `EmitEscape` retain going
    /// out, the resume value's own mint coming back, or `chan/send`'s send-site
    /// incref. So the second holder it creates is counted, as a lexical capture is.
    ///
    /// Read by the frame-held admission (`region::infer::escape::frame_held_regions`),
    /// which exists to exclude *uncounted* second holders.
    binding_escapes_containment: FxHashSet<Binding>,
    /// Allocation-site `HirId`s a value reaches a **tail/return** through: the
    /// region-level half of the return facet. These name the *atomless* escapes
    /// `binding_returns` cannot, such as a bare `(%pair …)` / `(@array …)` / call
    /// result / string literal at a tail, or a returned lambda. The region solver
    /// projects these through its `alloc_region` map; together with `binding_returns`
    /// (projected through `binding_source_regions`) they are the region-level return
    /// frontier. See `escapes_return_frontier`.
    return_frontier_sites: FxHashSet<HirId>,
    /// Allocation-site `HirId`s a value crosses the **fiber frontier** through —
    /// emitted (`yield`/`emit`) or sent (`chan/send`) — the atomless half of the
    /// fiber facet (`(yield (%pair 1 2))`). Projected through `alloc_region` by the
    /// region solver. See `escapes_fiber_frontier`.
    fiber_frontier_sites: FxHashSet<HirId>,
    /// Bindings whose value crosses the **fiber frontier** (emitted or sent) — the
    /// binding-level half of the fiber facet, projected through
    /// `binding_source_regions`. Distinct from the full `binding_escapes` (which
    /// also folds in store/capture containment), because only a frontier crossing —
    /// not containment — is an ownership Shared seed. See `escapes_fiber`.
    fiber_frontier_bindings: FxHashSet<Binding>,
}

impl EscapeInfo {
    /// The empty fact-set — nothing escapes. Identical to `Default`, named for
    /// symmetry with `RegionInfo::empty()` and to read as intent at call sites.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Does this binding's value escape its defining activation (any facet)? A
    /// binding not recorded as escaping defaults to `false`.
    pub fn binding_escapes_activation(&self, b: Binding) -> bool {
        self.binding_escapes.contains(&b)
    }

    /// Does this lambda's closure escape its definition? A lambda not recorded
    /// as escaping defaults to `false`.
    pub fn lambda_escapes_definition(&self, id: HirId) -> bool {
        self.lambda_escapes.contains(&id)
    }

    /// Does this binding's value escape via the **return facet** — flow to a
    /// tail/return position? Strictly narrower than `binding_escapes_activation`
    /// (a value stored into a container or captured by a closure escapes its
    /// activation but is *not* returned). The module-scope reassign gate's
    /// "not-returned" check reads this directly, per binding (see the field doc).
    pub fn binding_escapes_via_return(&self, b: Binding) -> bool {
        self.binding_returns.contains(&b)
    }

    /// Does this binding's value escape by a **containment** facet — a store into a
    /// longer-lived region, or capture by a closure that itself escapes? A fiber
    /// crossing is not one: it creates a counted holder, and a containment escape
    /// need not (see the field doc).
    pub fn binding_escapes_by_containment(&self, b: Binding) -> bool {
        self.binding_escapes_containment.contains(&b)
    }

    /// Does an allocation at this `HirId` reach a **tail/return** (the region-level
    /// return frontier)? The region solver tests this against each `alloc_region`
    /// key to project the atomless return escapes the binding facet cannot name.
    pub fn escapes_return_frontier(&self, id: HirId) -> bool {
        self.return_frontier_sites.contains(&id)
    }

    /// Does an allocation at this `HirId` cross the **fiber frontier** (emitted or
    /// sent)? Projected through `alloc_region` by the region solver.
    pub fn escapes_fiber_frontier(&self, id: HirId) -> bool {
        self.fiber_frontier_sites.contains(&id)
    }

    /// Does this binding's value cross the **fiber frontier** (emitted or sent)?
    /// The binding-level half of the fiber facet — narrower than
    /// `binding_escapes_activation`, which also counts store/capture containment.
    pub fn escapes_fiber(&self, b: Binding) -> bool {
        self.fiber_frontier_bindings.contains(&b)
    }
}

/// Compute escape facts over the canonical (functionalized + ANF) HIR.
///
/// Several seed sources, one shared backward propagation. Escape is a value-flow,
/// not a syntactic check, because it propagates *backward* through binding
/// definitions. Returning or storing a binding that holds a closure escapes that
/// closure (`(def f (fn …)) … f`), and an alias chain (`(let [g f] g)`) escapes
/// the original.
///
///   1. **Seed (return)** the atoms in a tail/return position: the top-level
///      expression's tail and every lambda body's tail. The walk descends the
///      region-transparent forms (`tail_sources`), and through an arg-returning
///      callee by the arg-return summary.
///   2. **Seed (store)** the atoms stored into a longer-lived region, which the
///      solver records as `cross_region_refs` sources. These are the operands of
///      the storing intrinsics (a pair every arg, a push or add arg 1, a put
///      arg 2), and of a native call whose `RegionEffect` declares a store.
///   3. **Seed (fiber boundary)** the value of each `Emit`, and each argument a
///      `Sends`/`Delivers` native hands across.
///   4. **Propagate** backward to a fixpoint over the binding-definition edges and
///      the capture edges, and for the containment facet alone over each binding's
///      pair contents too. The capture facet has no seed step: a value escapes via
///      capture only here, pulled in once a frontier seed marks its capturing
///      closure escaping.
///
/// The seed positions mirror the solver's value-flow walk, so the projection lines
/// up with the regions the lowerer emits.
pub fn analyze_escape(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &super::region::CallClassification,
) -> EscapeInfo {
    // Interprocedural return transparency: which fixed-param indices each
    // inlinable callee returns (the arg-return summary). `tail_sources` reads it
    // to descend through an arg-returning tail call, mirroring the solver's
    // inline.
    let arg_return = compute_arg_return(hir, arena);
    let ctx = TailCtx {
        arena,
        arg_return: &arg_return,
    };
    // The per-container stored-contents map: which values were stored into which
    // named container. A pre-pass builds it, so the read-result edge in
    // `collect_flow` sees every store wherever it sits relative to the read.
    let mut container_contents: FxHashMap<Binding, Vec<Atom>> = FxHashMap::default();
    collect_container_contents(&ctx, hir, call_class, &mut container_contents);
    let mut flow = Flow::default();
    // The top-level expression is the entry function's return value (return facet) —
    // both the atoms and the region-level allocation sites it reaches.
    return_atoms(&ctx, hir, &mut flow.return_seeds);
    record_frontier_sites(&ctx, hir, &mut flow.return_sites);
    collect_flow(&ctx, hir, call_class, &container_contents, &mut flow);
    let captures = Some(&flow.lambda_captures);

    // Return-only: just the return seeds, propagated backward through
    // binding-definition edges and NOT capture edges, because a captured value is
    // not *returned*. The reassign gate and the solver's return-frontier
    // projection read it.
    let returns = propagate(&flow.return_seeds, &flow.edges, None, None);
    let stored: Vec<Atom> = flow
        .store_seeds
        .iter()
        .chain(flow.pair_seeds.iter())
        .copied()
        .collect();
    // Containment alone: the uncounted stores, without the fiber seeds, followed
    // into the captures of an escaping closure and the operands of a contained pair.
    // A pair's own seeds stay out: it holds its operands by a counted edge (see
    // `binding_escapes_containment`).
    let containment = propagate(
        &flow.store_seeds,
        &flow.edges,
        captures,
        Some(&flow.pair_contents),
    );
    // Full escape: every facet's seeds, propagated through binding-definition AND
    // capture edges (a value captured by an escaping closure escapes too,
    // transitively).
    let mut all_seeds = flow.return_seeds.clone();
    all_seeds.extend(flow.fiber_seeds.iter().copied());
    all_seeds.extend(stored);
    let escaping = propagate(&all_seeds, &flow.edges, captures, None);

    let mut info = EscapeInfo::empty();
    for a in escaping {
        match a {
            Atom::Binding(b) => {
                info.binding_escapes.insert(b);
            }
            Atom::Lambda(id) => {
                info.lambda_escapes.insert(id);
            }
        }
    }
    // Only binding returns are recorded — the binding-level return facet's atom
    // half. Returned lambdas and atomless returns are carried by `return_sites`.
    for a in returns {
        if let Atom::Binding(b) = a {
            info.binding_returns.insert(b);
        }
    }
    for a in containment {
        if let Atom::Binding(b) = a {
            info.binding_escapes_containment.insert(b);
        }
    }
    // Fiber-frontier bindings: the directly emitted/sent binding seeds. No backward
    // propagation — the solver folds a binding's aliases into its
    // `binding_source_regions`, so projecting the direct seed already names the
    // crossing region.
    for a in &flow.fiber_seeds {
        if let Atom::Binding(b) = a {
            info.fiber_frontier_bindings.insert(*b);
        }
    }
    info.return_frontier_sites = flow.return_sites;
    info.fiber_frontier_sites = flow.fiber_sites;
    info
}

/// Backward reachability from `seeds` to a fixpoint.
///
/// - `edges` is always followed: an escaping binding pulls in the atoms its
///   definition flows from, its aliases.
/// - `captures`, when `Some`, is followed too: an escaping lambda pulls in every
///   binding it captures. The return-only set passes `None`, because a captured
///   value is not itself returned.
/// - `pairs`, when `Some`, is followed too: an escaping binding pulls in the
///   operands of each `%pair` it may hold. Only the containment facet asks.
fn propagate(
    seeds: &[Atom],
    edges: &FxHashMap<Binding, Vec<Atom>>,
    captures: Option<&FxHashMap<HirId, Vec<Binding>>>,
    pairs: Option<&FxHashMap<Binding, Vec<Atom>>>,
) -> FxHashSet<Atom> {
    let mut escaping: FxHashSet<Atom> = FxHashSet::default();
    let mut work: Vec<Atom> = Vec::new();
    let reach = |a: Atom, escaping: &mut FxHashSet<Atom>, work: &mut Vec<Atom>| {
        if escaping.insert(a) {
            work.push(a);
        }
    };
    for &a in seeds {
        reach(a, &mut escaping, &mut work);
    }
    while let Some(a) = work.pop() {
        match a {
            Atom::Binding(b) => {
                let from_pairs = pairs.and_then(|p| p.get(&b));
                for &s in edges.get(&b).into_iter().chain(from_pairs).flatten() {
                    reach(s, &mut escaping, &mut work);
                }
            }
            Atom::Lambda(l) => {
                for &b in captures.and_then(|c| c.get(&l)).into_iter().flatten() {
                    reach(Atom::Binding(b), &mut escaping, &mut work);
                }
            }
        }
    }
    escaping
}

#[cfg(test)]
mod tests;
