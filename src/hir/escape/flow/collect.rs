// audited: 2026-10-21
//! The backward-edge and seed collection the fixpoint drives, gathered into a [`Flow`] in one walk.
//!
//! docs/impl/escape.md
//!
//! `collect_flow` records the value-flow edges, the per-facet seeds (return,
//! fiber, store, pair), each binding's pair contents, and each lambda's captures.
//! `call_effect` classifies an opaque-call callee to match the region solver's
//! store/send edge seeding.

use rustc_hash::{FxHashMap, FxHashSet};

use super::atom::Atom;
use super::sources::{record_frontier_sites, return_atoms, tail_pair_operands, tail_sources};
use super::TailCtx;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};

/// The callee's declared `RegionEffect`, when the callee is an immutable,
/// unshadowed binding naming a declared primitive. It mirrors the region solver's
/// `RegionInference::call_effect`, so the store seeding matches the solver's
/// opaque-call edge classification. `None` for a user fn, a shadowed name, or a
/// non-`Var` callee.
fn call_effect(
    func: &Hir,
    arena: &BindingArena,
    call_class: &crate::hir::region::CallClassification,
) -> Option<crate::primitives::def::RegionEffect> {
    if let HirKind::Var(b) = &func.kind {
        let bi = arena.get(*b);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        call_class.effects.get(&bi.name).copied()
    } else {
        None
    }
}

/// The binding an expression is a bare reference to (`x`), else `None`. Used to
/// name the CONTAINER argument of a store/read op — a store into / read out of a
/// value the compiler can name by a binding.
fn var_binding(hir: &Hir) -> Option<Binding> {
    if let HirKind::Var(b) = &hir.kind {
        Some(*b)
    } else {
        None
    }
}

/// The container element-READ this expression is, named by its container binding
/// (arg0), if any: a `%first`/`%rest`/`%get` intrinsic, or a call to a declared
/// container-read native (`first`/`rest`/`get`/`pop` and their `%`-op peers,
/// `CallClassification::container_read_funnels`). `None` for anything else. Drives
/// the read-result → container-contents edge (see `collect_flow`).
fn read_container(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &crate::hir::region::CallClassification,
) -> Option<Binding> {
    match &hir.kind {
        HirKind::Intrinsic {
            op: IntrinsicOp::First | IntrinsicOp::Rest | IntrinsicOp::Get,
            args,
        } => args.first().and_then(var_binding),
        HirKind::Call { func, args, .. } => {
            let HirKind::Var(fb) = &func.kind else {
                return None;
            };
            let bi = arena.get(*fb);
            if bi.is_immutable
                && !bi.is_mutated
                && call_class.container_read_funnels.contains(&bi.name)
            {
                args.first().and_then(|a| var_binding(&a.expr))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// A value-retaining store into a named container: the container binding (arg0)
/// and the stored value expression (the last operand — arg 1 for `push`/`add`,
/// arg 2 for `put`). Covers both the intrinsic-node form (`Push`/`Put`/`AddSet`
/// and their monomorphic peers) and the `Funnel` call form (a call to a native in
/// `CallClassification::retaining_store_funnels`). `%pair` is excluded — it names
/// no container binding (it mints a fresh aggregate, not a store into a named one).
/// Drives the per-container stored-contents map (see `collect_flow`).
fn store_container_value<'h>(
    hir: &'h Hir,
    arena: &BindingArena,
    call_class: &crate::hir::region::CallClassification,
) -> Option<(Binding, &'h Hir)> {
    match &hir.kind {
        HirKind::Intrinsic { op, args } if is_named_store_op(*op) => {
            let container = args.first().and_then(var_binding)?;
            let value = args.last()?;
            Some((container, value))
        }
        HirKind::Call { func, args, .. } => {
            let HirKind::Var(fb) = &func.kind else {
                return None;
            };
            let bi = arena.get(*fb);
            if !(bi.is_immutable
                && !bi.is_mutated
                && call_class.retaining_store_funnels.contains(&bi.name))
            {
                return None;
            }
            let container = args.first().and_then(|a| var_binding(&a.expr))?;
            let value = args.last().map(|a| &a.expr)?;
            Some((container, value))
        }
        _ => None,
    }
}

/// The intrinsic-node store ops that name a container in arg0 (everything the
/// `retaining_store_funnels` call form covers, in opcode shape). NOT `%pair` (fresh
/// aggregate, no named container).
fn is_named_store_op(op: IntrinsicOp) -> bool {
    matches!(
        op,
        IntrinsicOp::Push
            | IntrinsicOp::PushArray
            | IntrinsicOp::PushArrayMut
            | IntrinsicOp::AddSet
            | IntrinsicOp::AddSetMut
            | IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut
    )
}

/// The per-container **stored-contents map**: for each named container binding,
/// the atoms of every value stored into it (via a `push`/`put`/`add` store op —
/// intrinsic or `Funnel` call). Built in one pre-pass over the whole tree so the
/// read-result → container-contents edge (`collect_flow`'s `add`) sees every store,
/// regardless of the store's position relative to the read.
///
/// This is the store half of the container-read-escape flow. A value read back
/// out of a container and escaped must not be adopted into the container's Owned
/// subtree (pinned by tests/impl/region-container-read-escape-uaf.lisp). The read edge marks
/// these contents escaping only when the read result itself escapes. A container
/// whose read result is consumed locally keeps its contents interior.
pub(in crate::hir::escape) fn collect_container_contents(
    ctx: &TailCtx,
    hir: &Hir,
    call_class: &crate::hir::region::CallClassification,
    out: &mut FxHashMap<Binding, Vec<Atom>>,
) {
    if let Some((container, value)) = store_container_value(hir, ctx.arena, call_class) {
        let mut srcs = Vec::new();
        tail_sources(ctx, value, &mut srcs);
        if !srcs.is_empty() {
            out.entry(container).or_default().extend(srcs);
        }
    }
    hir.for_each_child(|c| collect_container_contents(ctx, c, call_class, out));
}

/// What [`collect_flow`] gathers in one walk: the value-flow edges, each facet's
/// seeds, the atomless frontier sites, and the two containment relations a value
/// follows its holder through — a closure's captures and a pair's operands.
#[derive(Default)]
pub(in crate::hir::escape) struct Flow {
    /// Binding → the atoms its definitions flow from.
    pub(in crate::hir::escape) edges: FxHashMap<Binding, Vec<Atom>>,
    /// Tail positions: the return facet.
    pub(in crate::hir::escape) return_seeds: Vec<Atom>,
    /// Emit/send/deliver operands: the fiber facet.
    pub(in crate::hir::escape) fiber_seeds: Vec<Atom>,
    /// Operands a store hands to a holder the frame cannot count: a named store
    /// op, or a native that declares a store.
    pub(in crate::hir::escape) store_seeds: Vec<Atom>,
    /// Operands a `%pair` stores. Every facet but containment reads them as
    /// stores; containment reads them through `pair_contents` instead.
    pub(in crate::hir::escape) pair_seeds: Vec<Atom>,
    /// Binding → the operands of every `%pair` its value may be. The pair holds
    /// them by a counted edge, so they are contained only where the pair is.
    pub(in crate::hir::escape) pair_contents: FxHashMap<Binding, Vec<Atom>>,
    /// Allocation sites a value reaches a tail through with no atom to name it.
    pub(in crate::hir::escape) return_sites: FxHashSet<HirId>,
    /// Allocation sites a value crosses a fiber boundary through with no atom.
    pub(in crate::hir::escape) fiber_sites: FxHashSet<HirId>,
    /// Lambda → the bindings it captures, for the transitive capture consumer.
    pub(in crate::hir::escape) lambda_captures: FxHashMap<HirId, Vec<Binding>>,
}

/// Walk every node, recording everything the fixpoint needs into `flow`:
///
/// - the value-flow edges: each binding-introducing form maps its binding(s)
///   to the atoms its initializer flows from;
/// - each lambda's body-tail seeds, its own return;
/// - the store and pair seeds, and the `Emit` fiber-boundary seeds;
/// - each binding's pair contents, and each lambda's captured bindings.
///
/// The capture facet has **no seed of its own**. A value escapes via capture only
/// when `lambda_captures` pulls it in, once a frontier seed (return, store or
/// fiber) marks its capturing closure escaping. A closure never returned, stored
/// or sent is called in place and escapes nothing, so the lexical flag
/// `is_captured` seeds nothing here.
pub(in crate::hir::escape) fn collect_flow(
    ctx: &TailCtx,
    hir: &Hir,
    call_class: &crate::hir::region::CallClassification,
    container_contents: &FxHashMap<Binding, Vec<Atom>>,
    flow: &mut Flow,
) {
    let arena = ctx.arena;
    // A binding's value-flow edges: the atoms its initializer flows from. When the
    // initializer is a container element-READ (`(first c)`, `(get c k)`, …), they
    // also hold the atoms of everything stored into that container. So an escaping
    // read result pulls the container's stored contents into the same facet, which
    // refuses their adoption into the container's Owned subtree. The edge fires
    // only when the read result escapes; a read consumed locally seeds nothing. A
    // pair the initializer may evaluate to records its operands against the binding.
    let add = |b: Binding, init: &Hir, flow: &mut Flow| {
        let mut srcs = Vec::new();
        tail_sources(ctx, init, &mut srcs);
        if let Some(container) = read_container(init, arena, call_class) {
            if let Some(contents) = container_contents.get(&container) {
                srcs.extend(contents.iter().copied());
            }
        }
        if !srcs.is_empty() {
            flow.edges.entry(b).or_default().extend(srcs);
        }
        let mut stored = Vec::new();
        tail_pair_operands(ctx, init, &mut stored);
        if !stored.is_empty() {
            flow.pair_contents.entry(b).or_default().extend(stored);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                add(*b, init, flow);
            }
        }
        HirKind::Loop { bindings, .. } => {
            // Only the initial inits flow (the solver records `binding_regions`
            // from the init; `recur` re-binds but leaves `binding_regions`).
            for (b, init) in bindings {
                add(*b, init, flow);
            }
        }
        HirKind::Define { binding, value } => {
            add(*binding, value, flow);
        }
        HirKind::Assign {
            target: binding,
            value,
        } => {
            add(*binding, value, flow);
        }
        HirKind::SetCell { cell, value } => {
            if let HirKind::Var(b) = &cell.kind {
                add(*b, value, flow);
            }
        }
        HirKind::Destructure { pattern, value, .. } => {
            // A pattern binding aliases into the scrutinee's region(s), exactly
            // as the solver propagates `val_regions` to each bound var.
            for b in pattern.bindings().bindings {
                add(b, value, flow);
            }
        }
        HirKind::Match { value, arms } => {
            for (pat, _, _) in arms {
                for b in pat.bindings().bindings {
                    add(b, value, flow);
                }
            }
        }
        HirKind::Lambda(lambda) => {
            let body = &lambda.body;
            let captures = &lambda.captures;
            // The body's tail is the lambda's return (return facet) — both the
            // atoms (`return_atoms`) and the atomless allocation sites it reaches
            // (`record_frontier_sites`, the region-level half).
            return_atoms(ctx, body, &mut flow.return_seeds);
            record_frontier_sites(ctx, body, &mut flow.return_sites);
            // Record this lambda's upvalues for the transitive capture consumer:
            // if this lambda is later found escaping, every binding it captures
            // escapes with it.
            if !captures.is_empty() {
                flow.lambda_captures
                    .insert(hir.id, captures.iter().map(|c| c.binding).collect());
            }
        }
        // Store seeds. A value stored into an aggregate escapes its defining
        // activation. The region solver's `walk_intrinsic` records the same store
        // as a `cross_region_refs` edge from the value's region to the aggregate's.
        // Seed exactly the stored operands, the edge sources, and never the
        // aggregate, which does not escape by being written to.
        //
        // - `%pair`: every arg (car and cdr are both stored).
        // - The push and add ops: arg 1 (arg 0 is the collection).
        // - The put ops: arg 2 (arg 0 is the collection, arg 1 the key).
        HirKind::Intrinsic { op, args } => match op {
            IntrinsicOp::Pair => {
                for a in args {
                    tail_sources(ctx, a, &mut flow.pair_seeds);
                }
            }
            IntrinsicOp::Push
            | IntrinsicOp::PushArray
            | IntrinsicOp::PushArrayMut
            | IntrinsicOp::AddSet
            | IntrinsicOp::AddSetMut => {
                if let Some(v) = args.get(1) {
                    tail_sources(ctx, v, &mut flow.store_seeds);
                }
            }
            IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut => {
                if let Some(v) = args.get(2) {
                    tail_sources(ctx, v, &mut flow.store_seeds);
                }
            }
            _ => {}
        },
        // Fiber-boundary crossing: yield or emit. The emitted value is handed to
        // the resumer, a different activation and in general a different fiber, so
        // it escapes the emitting activation. An `Emit` has no compile-time RC
        // edge, because the runtime incref in `handle_emit` keeps the operand
        // alive. So escape alone records the crossing: the binding (`fiber_seeds`)
        // and the atomless allocation site (`fiber_sites`). A fiber body's return
        // value is already the return facet, since its tail is seeded as a lambda's.
        HirKind::Emit { value, .. } => {
            tail_sources(ctx, value, &mut flow.fiber_seeds);
            record_frontier_sites(ctx, value, &mut flow.fiber_sites);
        }
        // Native-declared store. A value a native stores, uncounted, into another
        // argument or an external structure escapes. These are the solver's
        // opaque-call `cross_region_refs` edge sources (`walk_call`). The seeds
        // follow the callee's declared `RegionEffect`:
        //   - `Stores{args}` seeds those args, the edge sources.
        //   - `Sends{args}` and `Delivers{args}` seed them on the fiber facet.
        //     Neither records an edge, because each seam counts its own
        //     reference, but the value still crosses to another fiber.
        //   - `Mixed` and `Unknown` seed every arg, since any arg may be stored.
        //   - Every other effect, and an opaque user fn (`None`), seeds nothing.
        //     `Opaque` copies every arg out and stores none.
        // So `chan/send` (`Sends{[1]}`) marks its message escaping, while
        // `fiber/new` and `chan/recv` (`Fresh`) do not: the spawned closure rides
        // the fresh fiber and escapes only if that fiber does. Under the default
        // classification, with no effects, every callee is `None` and nothing is
        // seeded.
        HirKind::Call { func, args, .. } => {
            use crate::primitives::def::RegionEffect;
            match call_effect(func, arena, call_class) {
                Some(RegionEffect::Stores { args: stored }) => {
                    // Store facet — the value goes into another arg / external
                    // structure (a containment edge), not across a frontier.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, &mut flow.store_seeds);
                        }
                    }
                }
                Some(
                    RegionEffect::Sends { args: stored } | RegionEffect::Delivers { args: stored },
                ) => {
                    // Fiber facet (send half) — `chan/send`'s message crosses to the
                    // receiving fiber. A `Delivers` install (`fiber/resume`'s resume
                    // value) goes into another fiber's signal slot. So both are
                    // frontier crossings (fiber seeds + region-level fiber sites),
                    // distinct from a `Stores` containment. Both are seam-counted
                    // (the send retain / the install's park-retain or transient
                    // handover), so the solver records no edge for either — which
                    // is not this analysis's question.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, &mut flow.fiber_seeds);
                            record_frontier_sites(ctx, &a.expr, &mut flow.fiber_sites);
                        }
                    }
                }
                Some(RegionEffect::Mixed | RegionEffect::Unknown) => {
                    for a in args {
                        tail_sources(ctx, &a.expr, &mut flow.store_seeds);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    hir.for_each_child(|c| collect_flow(ctx, c, call_class, container_contents, flow));
}
