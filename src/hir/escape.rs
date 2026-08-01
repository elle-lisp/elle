//! Escape analysis over the canonical (functionalized + ANF) HIR.
//!
//! The **single authority** for whether a value outlives the activation it was born
//! in — computed once over the regularized IR. Every consumer that needs the
//! property reads it rather than recomputing a proxy: the region solver (the Owned-
//! vs-Shared classifier, the reassign gate, the merge gate, branch compensation —
//! all through the region projection in `region::infer::escape`) and the lowerer's
//! tail-call ownership predicates. There is no parallel escape judgment anywhere
//! else: the former lexical proxy (`is_captured`) is demoted to a structural hint
//! (below), and the region solver records no escape facts of its own — it projects
//! this analysis's verdict onto regions through `region::infer::escape`.
//!
//! ## The two questions it answers
//!
//! - **per binding** — does the binding's value *escape its defining
//!   activation*? (`binding_escapes_activation`)
//! - **per lambda** — does the closure *escape its definition*?
//!   (`lambda_escapes_definition`)
//!
//! plus a complementary pair that splits the per-binding answer on the **return**
//! facet — `binding_escapes_via_return` and `binding_escapes_beyond_return` — so a
//! consumer can ask "by the return facet and no other", which neither the full set
//! nor either half can express alone.
//!
//! ## The four escape facets
//!
//! Escape is a value-flow over the atoms (binding / lambda) the region solver's
//! `walk` tracks:
//!
//! - **return** — a binding/lambda escapes when its value reaches a function's
//!   tail/return position. (This also covers a fiber's *terminal* value: a fiber
//!   body is a lambda whose tail is seeded, and that value crosses to the joiner.)
//! - **store** — a binding/lambda escapes when its value is *stored into a
//!   longer-lived region*. Two store sources: the allocating **intrinsics**
//!   (`(%pair v …)`, `(%array-push coll v)`, `(%put obj k v)` — the value embeds in
//!   the fresh aggregate), and **native calls that declare a store** via their
//!   `RegionEffect` (read from the `CallClassification`): `Stores{args}`/`Sends{args}`
//!   escape those args, `Mixed`/`Unknown` escapes every arg, and `Fresh`/`Immediate`/
//!   `PassThrough`/`Funnel` escape nothing. This is how `chan/send` (`Sends{[1]}`)
//!   marks its message escaping — the *send* fiber boundary — while `fiber/new`/
//!   `chan/recv` (`Fresh`) do not. (`Sends` and `Stores` escape a binding identically;
//!   they differ only in that `Sends` ALSO crosses the fiber frontier, which the
//!   fiber facet records — see below.)
//! - **capture** — a value *captured by a closure that itself escapes* escapes
//!   too, transitively. The capture facet has **no seed of its own**: a closure
//!   escapes its definition ONLY when its value returns/stores/crosses a fiber
//!   boundary (the frontier facets above). Each escaping closure then propagates
//!   its escape to every binding it captures (`lambda_captures`), transitively (a
//!   captured binding may be a lambda whose own captures escape). A closure that
//!   is captured but never crosses a frontier is called in place and escapes
//!   nothing, so the lexical-capture proxy `is_captured` seeds escape NOWHERE
//!   (precision-point-3 made flow-true).
//! - **fiber boundary** — a value handed across a fiber boundary escapes:
//!   - *yield/emit* — an `Emit` node's value is delivered to the resumer.
//!   - *terminal value* — the return facet (a fiber body is a lambda whose tail
//!     is seeded, and that value crosses to the joiner).
//!   - *send* — the store facet: `chan/send` declares `Sends{[1]}` (a `Stores` that
//!     also crosses the fiber frontier), so its message escapes (above).
//!   - *spawn* — `fiber/new` is `Fresh`: the spawned closure rides the fresh fiber
//!     result and escapes only if that result does (the ordinary result-flow), so
//!     it needs no separate rule. (`ev/spawn` is a stdlib fn, so the closure's
//!     escape is accounted in its own compilation.)
//!
//! All four facets seed atoms (see the function doc), then propagate backward to a
//! fixpoint through the binding-definition edges and the capture edges, so an
//! alias of an escaping value — and a value captured by an escaping closure —
//! escapes too.
//!
//! ## The region-level frontier (atomless escapes)
//!
//! The two **frontier** facets — **return** and **fiber** (emit/send) — are the
//! ownership Shared seeds, and a value can cross them with no binding/lambda atom to
//! name it: `(yield (%pair 1 2))`, a bare aggregate at a tail. So alongside the atom
//! sets, the frontier facets also record the **allocation-site `HirId`s** they reach
//! (`record_frontier_sites`): `return_frontier_sites` / `fiber_frontier_sites`. The
//! region solver projects these — and the atom-level facets — onto regions through
//! its own `alloc_region` / `binding_source_regions` maps (`region::infer::escape`). Escape
//! never sees a region; the projection is the consumer's.
//!
//! ## Consumers
//!
//! Every consumer reads this analysis; none keeps a parallel judgment.
//!
//! - **The region solver** (`region::infer::escape` projects the verdict to regions):
//!   the ownership **Shared seed** (`compute_shared_seeds` = the return ∪ fiber
//!   frontier), the **merge** gate's not-returned check (`returned_regions` = the
//!   return frontier; its separate sole-held *reachability* refusal reads the
//!   region capture-graph `region::infer::escape::captured_bindings`, never escape and
//!   never the `is_captured` proxy), branch **compensation**'s escaping-exclusion
//!   (the return frontier), and the reassign 1-slot-container gate
//!   (`binding_escapes_via_return`, per binding).
//! - **The lowerer** (`lir/lower`): `tail_callee_defers_release` reads
//!   `lambda_escapes_definition` / `binding_escapes_activation` for the escape half
//!   of the deferral decision — region-locality stays a region fact. For a
//!   **stranded recursive** callee, whose region has no other release channel, it
//!   narrows to `escapes_fiber` alone: the store/capture facets are containment, and
//!   the return facet is funded by the callee's own return mint, which precedes the
//!   deferred release (docs/impl/selfrec.md).
//!
//! Two lowerer/HIR decisions deliberately do **not** read this analysis, because
//! the question they answer is *ownership-location / mutation-sharing*, which is
//! structural lexical capture, not true-escape:
//!  - `tail_arg_is_borrowed` (`lir/lower/control.rs`): a tail-arg is borrowed iff
//!    its binding is a captured upvalue (the env owns the capture-incref). Escape
//!    over-approximates this (a born-here value that flows to a tail *escapes* but
//!    is *owned*), and minting for those owned-escaping args double-releases
//!    across a fiber suspend/resume — a phantom `DecrefRegion`/UAF (witnessed on
//!    `contracts.lisp`). So it reads `upvalue_bindings` (structural capture).
//!  - cell insertion in `functionalize` / the lowerer's closure-env layout: a
//!    captured *mutable* binding needs a shared cell even when its capturing
//!    closure never escapes, so this reads `needs_capture` — for a local,
//!    `is_captured ∧ (¬immutable ∨ is_prebound)`: a captured *mutable* local, **or** a
//!    *prebound* immutable one (a recursive `letrec`'s forward reference, the carve-out
//!    the closure-cycle merge relies on); for a param, `is_mutated` — not escape.
//!
//! These two are the *structural-only* role of lexical capture, and the **only**
//! roles left to `is_captured`: it feeds NO escape facet (the capture facet is
//! flow-true — pure transitive `lambda_captures` propagation from genuine frontier
//! seeds), and it is module-private with no getter, so no consumer can re-couple
//! the solver to it.
//!
//! ## Verification
//!
//! Escape is the authority, so it is pinned by its **own** four-facet spec, not by
//! agreement with another analysis. Three layers: the unit tests (`tests/`) assert
//! each facet's discriminating behaviour directly (a returned value return-escapes;
//! a stored value escapes-but-is-not-returned; a value captured by a non-escaping
//! closure does not escape; an emitted/sent value crosses the fiber frontier); the
//! escape golden (`tests/elle/escape-golden.lisp`) pins the normalized dump — the
//! return-frontier projection and the RC instructions it drives — on real corpus
//! files; and the region suite + `oracle.lisp` prove the projection reclaims
//! soundly (no UAF, no leak regression).
//!
//! ## Interprocedural return transparency
//!
//! The return facet is **interprocedural** for an *arg-returning* callee. A tail
//! call `(id y)` to a function that returns its parameter is region-transparent in
//! that argument: the call yields whatever flowed into the arg, so `y` escapes
//! when the call's result does.
//!
//! It is realized as an **arg-return summary** (`compute_arg_return`): per
//! inlinable lambda binding, which fixed-param indices flow to its tail, computed
//! to a fixpoint (so `(fn (z) (id z))` chains through `id`'s summary). `tail_sources`
//! consumes it at a `Call` — descending into the returned-arg positions instead of
//! treating the call as opaque. "Inlinable" is an immutable, unmutated binding
//! bound to a `Lambda` by a `Let`/`Letrec` — **never** a top-level `Define` (a
//! `def`-bound callee stays opaque, and an arg returned through it does not escape
//! via the call).
//!
//! ## Precision characteristics
//!
//! Escape is finer than the structural proxies it replaces; these are the
//! characterized points where its verdict is the *precise* one (each pinned by a
//! test):
//! - *Stored borrowed param.* A stored borrowed param — `(fn (x) (%pair x x))`, or
//!   a `chan/send` whose message is a param — genuinely escapes (it embeds in a
//!   longer-lived aggregate / crosses a fiber), so the store facet marks it. A
//!   param's region is a runtime placeholder the region projection treats as
//!   not-ownable, so marking it is sound and costs the consumers nothing.
//! - *Store target vs stored value.* `(%put obj k v)` / `(assign acc v)` escapes the
//!   stored *value*, never the *container* `obj`/`acc` (writing into a container is
//!   not the container escaping). Escape marks only the value; the projection acts
//!   per region, so the value's region is marked exactly once.
//! - *Lexical capture is not escape.* A value captured by a closure that is *called
//!   in place* (never escapes) does not escape via capture — the capture facet marks
//!   it only when its capturing closure escapes, where `is_captured` marks every
//!   captured binding unconditionally. (A value the closure *returns* still
//!   return-escapes through the closure's own tail — a separate facet.)
//! - *Fiber boundary.* An emitted/sent value crosses to the resumer/receiver, so the
//!   fiber facet marks it (and the region-level `fiber_frontier_sites` catch an
//!   atomless `(yield (%pair …))`). There is no compile-time RC edge at an `Emit`
//!   (the runtime incref in `handle_emit` keeps it alive) — the fiber crossing is
//!   purely escape's to record.
//! - *Native `Mixed`/`Unknown` clique is conservative.* A native declared `Mixed`
//!   (uncounted store, examined) or `Unknown` (unexamined — the default) marks
//!   *every* heap argument escaping. Sound; as imprecise as the declarations are
//!   honest. Examining a primitive and declaring a tighter `RegionEffect` narrows
//!   it.

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
    /// Read by the frame-exit release's return-funded admission
    /// (docs/impl/region/mechanism.md § "The callee's return mint, and the edge
    /// that funds the gap"), which replaces the return facet's refusal with the
    /// tail callee's own counted edge and must therefore know no other facet is
    /// also refusing.
    binding_escapes_beyond: FxHashSet<Binding>,
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
    /// activation but is *not* returned). The reassign gate's "not-returned"
    /// check reads this directly, per binding (see the field doc).
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
///      through an arg-returning callee (the arg-return summary; see "Interprocedural
///      return transparency" in the module doc).
///   2. **Seed (store)** the atoms stored into a longer-lived region — the operands
///      the solver records as `cross_region_refs` sources. Two sources: the
///      allocating intrinsics (`collect_flow`'s `Intrinsic` arm — `%pair` every
///      arg, `%array-push` arg 1, `%put` arg 2) and **native calls that declare a
///      store** (`collect_flow`'s `Call` arm, keyed on the callee's `RegionEffect`
///      from `call_class`): `Stores{args}`/`Sends{args}` seed those args,
///      `Mixed`/`Unknown` seeds every arg (the solver's mutual clique), and
///      `Fresh`/`Immediate`/`PassThrough`/`Funnel` seed nothing. This is how
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
///      seed marks its capturing closure escaping (precision-point-3, flow-true).
///
/// The seed positions mirror the solver's value-flow walk (so the projection lines
/// up with the regions the lowerer emits): the binding-definition edges parallel
/// `binding_source_regions` copying an init's regions, and the store seeds parallel
/// the `cross_region_refs` edges recorded at the same intrinsics/native calls. The
/// facets and their precision characteristics are documented in the module doc.
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
    // inline (see the module doc).
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
    // Fiber-frontier bindings: the directly emitted/sent binding seeds. No backward
    // propagation — the solver folds a binding's aliases into its
    // `binding_source_regions`, so projecting the direct seed already names the
    // crossing region (matching the former region-level `emit`/`send` seeds).
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
