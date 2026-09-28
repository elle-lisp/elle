// audited: 2026-09-28
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
    return_atoms, Atom, TailCtx,
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
    /// This is what the reassign 1-slot-container gate's "not-returned" check
    /// reads — the gate refuses the optimization for a *returned* value (two
    /// static owners of one transferred reference) while *keeping* it for a value
    /// that merely stores into a container or is captured (those are
    /// runtime-counted), so the *return* facet, not the full escape set, is the
    /// right question. It is read **per binding** (atom-level). Together with
    /// `return_frontier_sites` it is the return half of the ownership Shared seed,
    /// projected to regions by `region::infer::escape` — precise for a cell that merely
    /// *points at* a region some function returns without itself flowing to a tail
    /// (the value genuinely is not returned).
    binding_returns: FxHashSet<Binding>,
    /// Bindings whose value escapes by some facet **other than** return — store,
    /// capture, or fiber. The complement of `binding_returns` within the full set,
    /// and the two together are what let a consumer ask "does this value escape by
    /// the return facet *and no other*". `binding_escapes` alone cannot answer
    /// that: a value both returned and yielded is in it once, indistinguishable
    /// from one that is only returned.
    ///
    /// Unlike `binding_returns` this DOES propagate through capture edges, because
    /// the facets it carries are the ones a closure's escape genuinely transmits.
    /// A closure that leaves only by being *returned* seeds nothing here, which is
    /// the reading its consumer needs: that closure's hold on its captures is the
    /// funnel's counted edge, and whatever it carries out is the return facet's
    /// business.
    ///
    /// Read by the frame-held admission the branch-arm window and the frame-exit
    /// release share (docs/impl/region/relocate.md), which admits the return facet
    /// and must therefore know no other facet is also refusing.
    binding_escapes_beyond: FxHashSet<Binding>,
    /// Bindings whose value escapes by a **containment** facet — stored into a
    /// longer-lived region, or captured by a closure that itself escapes. The
    /// beyond-return set less its fiber half, propagated through both edge kinds.
    ///
    /// The split exists because the two groups answer the *holder* question
    /// differently. A containment escape hands the value to a holder the frame
    /// cannot see and, for a declared native store, cannot count. A fiber crossing
    /// hands it to a seam that counts its own reference — the park's `EmitEscape`
    /// retain going out, the resume value's own mint coming back, `chan/send`'s
    /// send-site incref — so the second holder it creates is counted, exactly as a
    /// lexical capture's is.
    /// Read by the frame-held admission (`region::infer::escape::frame_held_regions`),
    /// which exists to exclude *uncounted* second holders.
    binding_escapes_containment: FxHashSet<Binding>,
    /// Allocation-site `HirId`s a value reaches a **tail/return** through — the
    /// region-level half of the return facet, naming the *atomless* escapes
    /// `binding_returns` cannot (a bare `(%pair …)` / `(@array …)` / call result /
    /// string literal at a tail, or a returned lambda). The region solver projects
    /// these through its `alloc_region` map; together with `binding_returns`
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

    /// Does this binding's value escape by a facet **other than** return — store,
    /// capture, or fiber? The complement of `binding_escapes_via_return`; together
    /// they express "escapes by the return facet and no other", which neither the
    /// full set nor the return-only set can say alone (see the field doc).
    pub fn binding_escapes_beyond_return(&self, b: Binding) -> bool {
        self.binding_escapes_beyond.contains(&b)
    }

    /// Does this binding's value escape by a **containment** facet — a store into a
    /// longer-lived region, or capture by a closure that itself escapes? The
    /// beyond-return set less its fiber half (see the field doc): a fiber crossing
    /// creates a counted holder, a containment escape need not.
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
/// definitions: returning/storing a binding that holds a closure escapes that
/// closure (`(def f (fn …)) … f`), and an alias chain (`(let [g f] g)`) escapes
/// the original.
///
///   1. **Seed (return)** the atoms in a tail/return position — the top-level
///      expression's tail and every lambda body's tail, descended through the
///      region-transparent forms (`tail_sources`), *including interprocedurally*
///      through an arg-returning callee (the arg-return summary;
///      docs/impl/escape.md).
///   2. **Seed (store)** the atoms stored into a longer-lived region — the operands
///      the solver records as `cross_region_refs` sources. Two sources: the
///      allocating intrinsics (`collect_flow`'s `Intrinsic` arm — `%pair` every
///      arg, `%array-push` arg 1, `%put` arg 2) and **native calls that declare a
///      store** (`collect_flow`'s `Call` arm, keyed on the callee's `RegionEffect`
///      from `call_class`): `Stores{args}` seeds those args, `Sends{args}` and
///      `Delivers{args}` seed them on the fiber facet (a seam-counted frontier
///      crossing, not an edge source), `Mixed`/`Unknown` seeds every arg (the
///      solver's mutual clique), and `Fresh`/`Immediate`/`PassThrough`/`Funnel`/
///      `Opaque` seed nothing. This is how
///      `chan/send` (`Sends{[1]}`) marks its message escaping while `fiber/new`
///      (`Fresh` — the closure rides the fresh fiber result) and `chan/recv`
///      (`Fresh`) do not.
///   3. **Seed (fiber boundary)** the value of each `Emit` (yield/emit), handed to
///      the resumer.
///   4. **Propagate** backward to a fixpoint over two edge kinds: binding-definition
///      edges (an escaping binding pulls in the atoms its definition flows from —
///      `edges`, collected over Let/Letrec/Loop/Define/Destructure/Match/Assign/
///      SetCell) and capture edges (an escaping lambda pulls in every binding it
///      captures — `lambda_captures`). The **capture facet has no seed step**: a
///      value escapes via capture only here, pulled in transitively once a frontier
///      seed marks its capturing closure escaping.
///
/// The seed positions mirror the solver's value-flow walk (so the projection lines
/// up with the regions the lowerer emits): the binding-definition edges parallel
/// `binding_source_regions` copying an init's regions, and the store seeds parallel
/// the `cross_region_refs` edges recorded at the same intrinsics/native calls. The
/// facets and their precision characteristics are documented in docs/impl/escape.md.
pub fn analyze_escape(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &super::region::CallClassification,
) -> EscapeInfo {
    let mut edges: FxHashMap<Binding, Vec<Atom>> = FxHashMap::default();
    // Seeds split by facet. `return_seeds` — tail positions (the return facet).
    // `fiber_seeds` — emit/send (the fiber facet), kept separate from `other_seeds`
    // (store + capture containment) so the fiber-only binding set can be derived: a
    // frontier crossing is an ownership Shared seed, containment is not. The full
    // escape set unions all three.
    let mut return_seeds: Vec<Atom> = Vec::new();
    let mut fiber_seeds: Vec<Atom> = Vec::new();
    let mut other_seeds: Vec<Atom> = Vec::new();
    // Region-level half of the frontier facets: the allocation-site `HirId`s a
    // value reaches a tail (`return_sites`) or a fiber boundary (`fiber_sites`)
    // through with no binding/lambda atom to name it. The region solver projects
    // these through `alloc_region`.
    let mut return_sites: FxHashSet<HirId> = FxHashSet::default();
    let mut fiber_sites: FxHashSet<HirId> = FxHashSet::default();
    // Lambda HirId → the bindings it captures (its upvalues). Drives the
    // transitive capture consumer in the full fixpoint below.
    let mut lambda_captures: FxHashMap<HirId, Vec<Binding>> = FxHashMap::default();
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
    // named container. Built in one pre-pass so the read-result → container-contents
    // edge (`collect_flow`) sees every store regardless of its position relative to
    // the read. The store half of the container-read-escape flow.
    let mut container_contents: FxHashMap<Binding, Vec<Atom>> = FxHashMap::default();
    collect_container_contents(&ctx, hir, call_class, &mut container_contents);
    // The top-level expression is the entry function's return value (return facet) —
    // both the atoms and the region-level allocation sites it reaches.
    return_atoms(&ctx, hir, &mut return_seeds);
    record_frontier_sites(&ctx, hir, &mut return_sites);
    collect_flow(
        &ctx,
        hir,
        call_class,
        &container_contents,
        &mut edges,
        &mut return_seeds,
        &mut fiber_seeds,
        &mut other_seeds,
        &mut return_sites,
        &mut fiber_sites,
        &mut lambda_captures,
    );

    // Return-only: just the return seeds, propagated backward through
    // binding-definition edges and NOT capture edges — a captured value is not
    // *returned*. The binding-level half of the return frontier; the reassign gate
    // and the solver's return-frontier projection read it.
    let returns = propagate(&return_seeds, &edges, None);
    // Beyond-return: every facet EXCEPT return, propagated through both edge kinds.
    // The complement that makes "returned and nothing else" expressible; a closure
    // escaping only by return seeds nothing here, so its captures are not pulled in
    // (see `binding_escapes_beyond`).
    let mut beyond_seeds = fiber_seeds.clone();
    beyond_seeds.extend(other_seeds.iter().copied());
    let beyond = propagate(&beyond_seeds, &edges, Some(&lambda_captures));
    // Containment alone: the store/capture half of beyond-return, without the fiber
    // seeds. The frame-held admission reads this rather than `beyond`, because a
    // fiber crossing counts its own reference at the seam and so is not the
    // uncounted second holder that admission refuses (see `binding_escapes_containment`).
    let containment = propagate(&other_seeds, &edges, Some(&lambda_captures));
    // Full escape: every facet's seeds, propagated through binding-definition AND
    // capture edges (a value captured by an escaping closure escapes too,
    // transitively).
    let mut all_seeds = return_seeds;
    all_seeds.extend(fiber_seeds.iter().copied());
    all_seeds.extend(other_seeds);
    let escaping = propagate(&all_seeds, &edges, Some(&lambda_captures));

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
    for a in beyond {
        if let Atom::Binding(b) = a {
            info.binding_escapes_beyond.insert(b);
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
    for a in &fiber_seeds {
        if let Atom::Binding(b) = a {
            info.fiber_frontier_bindings.insert(*b);
        }
    }
    info.return_frontier_sites = return_sites;
    info.fiber_frontier_sites = fiber_sites;
    info
}

/// Backward reachability from `seeds` to a fixpoint. Always follows
/// binding-definition edges (`edges`: an escaping binding pulls in the atoms its
/// definition flows from — aliases). Follows capture edges (`captures`: an
/// escaping lambda pulls in every binding it captures) only when `captures` is
/// `Some`: the full escape set propagates capture (a value captured by an
/// escaping closure escapes), the return-only set does not (a captured value is
/// not itself returned — matching `returned_regions`, which records no
/// captured-by-returned-closure region).
fn propagate(
    seeds: &[Atom],
    edges: &FxHashMap<Binding, Vec<Atom>>,
    captures: Option<&FxHashMap<HirId, Vec<Binding>>>,
) -> FxHashSet<Atom> {
    let mut escaping: FxHashSet<Atom> = FxHashSet::default();
    let mut work: Vec<Atom> = Vec::new();
    for &a in seeds {
        if escaping.insert(a) {
            work.push(a);
        }
    }
    while let Some(a) = work.pop() {
        match a {
            Atom::Binding(b) => {
                if let Some(srcs) = edges.get(&b) {
                    for &s in srcs {
                        if escaping.insert(s) {
                            work.push(s);
                        }
                    }
                }
            }
            Atom::Lambda(l) => {
                if let Some(caps) = captures.and_then(|c| c.get(&l)) {
                    for &b in caps {
                        let cap = Atom::Binding(b);
                        if escaping.insert(cap) {
                            work.push(cap);
                        }
                    }
                }
            }
        }
    }
    escaping
}

#[cfg(test)]
mod tests;
