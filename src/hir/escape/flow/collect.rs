//! The backward-edge and seed collection the fixpoint drives: `collect_flow`
//! walks every node once, recording the value-flow edges and the per-facet seeds
//! (return, fiber, store), plus each lambda's captures. `call_effect` classifies
//! an opaque-call callee to match the region solver's store/send edge seeding.

use rustc_hash::{FxHashMap, FxHashSet};

use super::atom::Atom;
use super::sources::{record_frontier_sites, return_atoms, tail_sources};
use super::TailCtx;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};

/// The callee's declared `RegionEffect`, when the callee is an immutable,
/// unshadowed binding naming a declared primitive — mirrors the region solver's
/// `RegionInference::call_effect` (`regions/walk.rs`) so the store seeding matches
/// the solver's opaque-call edge classification. `None` for a user fn, a shadowed
/// name, or a non-`Var` callee.
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
/// This is the store half of the container-read-escape flow: a value read back out
/// of a container and escaped must not be adopted into the container's Owned subtree
/// (pinned by `region_container_read_escape_uaf`). The read edge marks these
/// exact contents escaping — and only when the read result itself escapes, so a
/// container merely read/indexed with the result consumed locally keeps its contents
/// interior (the precise gate the fixpoint gives for free).
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

/// Walk every node, recording everything the fixpoint needs: the value-flow
/// edges (each binding-introducing form maps its binding(s) to the atoms its
/// initializer flows from), each lambda's body-tail seeds (its own return),
/// store seeds (allocating intrinsics and native-declared stores), the `Emit`
/// fiber-boundary seeds, and each lambda's captured bindings (`lambda_captures`,
/// for the transitive capture consumer).
///
/// The capture facet has **no seed of its own**: a value escapes via capture only
/// by being pulled in transitively through `lambda_captures` once a genuine
/// frontier seed (return / store / fiber) marks its capturing closure escaping
/// (escape.md precision-point-3 — "the capture facet marks a value only when its
/// capturing closure escapes"). A closure captured but never returned/stored/sent
/// is called in place and escapes nothing, so the lexical-capture proxy
/// `is_captured` seeds escape nowhere here.
// A recursive accumulator-walk: each `&mut` sink collects one fixpoint input, kept
// as a distinct parameter rather than bundled so the arms read straightforwardly.
#[allow(clippy::too_many_arguments)]
pub(in crate::hir::escape) fn collect_flow(
    ctx: &TailCtx,
    hir: &Hir,
    call_class: &crate::hir::region::CallClassification,
    container_contents: &FxHashMap<Binding, Vec<Atom>>,
    edges: &mut FxHashMap<Binding, Vec<Atom>>,
    return_seeds: &mut Vec<Atom>,
    fiber_seeds: &mut Vec<Atom>,
    other_seeds: &mut Vec<Atom>,
    return_sites: &mut FxHashSet<HirId>,
    fiber_sites: &mut FxHashSet<HirId>,
    lambda_captures: &mut FxHashMap<HirId, Vec<Binding>>,
) {
    let arena = ctx.arena;
    // A binding's value-flow edges: the atoms its initializer flows from. PLUS, when
    // the initializer is a container element-READ (`(first c)`/`(get c k)`/…), the
    // atoms of everything stored into that container — the read-result →
    // container-contents edge. So an escaping read result pulls the container's stored
    // contents into the same facet, refusing their adoption into the container's Owned
    // subtree (pinned by `region_container_read_escape_uaf`). The edge fires
    // through the ordinary fixpoint ONLY when the read result actually escapes — a
    // container read with the result consumed locally seeds nothing, so its contents
    // stay interior (the precise gate, not the naive "every read escapes").
    let add = |b: Binding, init: &Hir, edges: &mut FxHashMap<Binding, Vec<Atom>>| {
        let mut srcs = Vec::new();
        tail_sources(ctx, init, &mut srcs);
        if let Some(container) = read_container(init, arena, call_class) {
            if let Some(contents) = container_contents.get(&container) {
                srcs.extend(contents.iter().copied());
            }
        }
        if !srcs.is_empty() {
            edges.entry(b).or_default().extend(srcs);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                add(*b, init, edges);
            }
        }
        HirKind::Loop { bindings, .. } => {
            // Only the initial inits flow (the solver records `binding_regions`
            // from the init; `recur` re-binds but leaves `binding_regions`).
            for (b, init) in bindings {
                add(*b, init, edges);
            }
        }
        HirKind::Define { binding, value } => {
            add(*binding, value, edges);
        }
        HirKind::Assign {
            target: binding,
            value,
        } => {
            add(*binding, value, edges);
        }
        HirKind::SetCell { cell, value } => {
            if let HirKind::Var(b) = &cell.kind {
                add(*b, value, edges);
            }
        }
        HirKind::Destructure { pattern, value, .. } => {
            // A pattern binding aliases into the scrutinee's region(s), exactly
            // as the solver propagates `val_regions` to each bound var.
            for b in pattern.bindings().bindings {
                add(b, value, edges);
            }
        }
        HirKind::Match { value, arms } => {
            for (pat, _, _) in arms {
                for b in pat.bindings().bindings {
                    add(b, value, edges);
                }
            }
        }
        HirKind::Lambda { body, captures, .. } => {
            // The body's tail is the lambda's return (return facet) — both the
            // atoms (`return_atoms`) and the atomless allocation sites it reaches
            // (`record_frontier_sites`, the region-level half).
            return_atoms(ctx, body, return_seeds);
            record_frontier_sites(ctx, body, return_sites);
            // Record this lambda's upvalues for the transitive capture consumer:
            // if this lambda is later found escaping, every binding it captures
            // escapes with it.
            if !captures.is_empty() {
                lambda_captures.insert(hir.id, captures.iter().map(|c| c.binding).collect());
            }
        }
        // Store seeds — store-into-a-longer-lived-region escape. A value stored
        // into a freshly-allocated aggregate escapes its defining activation: the
        // region solver records the same as a `cross_region_refs` edge
        // `src=value-region → dst=aggregate-region` at these very intrinsics
        // (regions/walk/walkrest.rs's `Intrinsic` arm). Seed exactly the operands
        // that are those
        // edge SOURCES (the stored values) — never the aggregate (the store
        // target / edge destination, which does not escape by being written to):
        //   %pair       — every arg (car and cdr are both stored into the pair)
        //   %array-push — arg 1 (arg 0 is the collection / target)
        //   %put        — arg 2 (arg 0 is the collection, arg 1 the key)
        HirKind::Intrinsic { op, args } => match op {
            IntrinsicOp::Pair => {
                for a in args {
                    tail_sources(ctx, a, other_seeds);
                }
            }
            IntrinsicOp::Push
            | IntrinsicOp::PushArray
            | IntrinsicOp::PushArrayMut
            | IntrinsicOp::AddSet
            | IntrinsicOp::AddSetMut => {
                if let Some(v) = args.get(1) {
                    tail_sources(ctx, v, other_seeds);
                }
            }
            IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut => {
                if let Some(v) = args.get(2) {
                    tail_sources(ctx, v, other_seeds);
                }
            }
            _ => {}
        },
        // Fiber-boundary crossing — yield/emit. The emitted value is handed to the
        // resumer (a different activation, in general a different fiber), so it
        // escapes the emitting activation. There is no compile-time RC edge at an
        // `Emit` (the runtime incref in `handle_emit` keeps the operand alive past
        // the resume-site decref), so the fiber crossing is purely escape's to
        // record — the binding (`fiber_seeds`) and the atomless allocation site
        // (`fiber_sites`). The terminal-value boundary (a fiber body's return
        // crossing to the joiner) is already the return facet — a fiber body is a
        // lambda whose tail is seeded.
        HirKind::Emit { value, .. } => {
            // Fiber-facet seeds — kept separate from `other_seeds` (store/capture)
            // so the fiber-only binding set and the region-level fiber frontier can
            // be derived. Both the atoms and the atomless allocation sites.
            tail_sources(ctx, value, fiber_seeds);
            record_frontier_sites(ctx, value, fiber_sites);
        }
        // Native-declared store — a value passed to a native that stores it
        // (uncounted) into another argument or an external structure escapes,
        // exactly the solver's opaque-call `cross_region_refs` edge sources
        // (`regions/walk/walkrest.rs`'s `Call` arm). Keyed on the callee's declared
        // `RegionEffect`: `Stores{args}`/`Sends{args}` seed those args (the edge
        // sources); `Mixed`/`Unknown` seeds every arg (the solver's full mutual
        // clique — any arg may be stored); `Fresh`/`Immediate`/`PassThrough`/`Funnel`/
        // `Opaque` and an opaque user fn (`None`) seed nothing (no uncounted store the
        // caller must account for — `Opaque` copies every arg out, storing none). This is how `chan/send` (`Sends{[1]}`) marks its message
        // escaping while `fiber/new`/`chan/recv` (`Fresh`) do not — the spawned
        // closure rides the fresh fiber result and escapes only if that result
        // does. (`ev/spawn` is a user fn, so its closure's escape is accounted in
        // its own compilation.) Under the default classification — empty effects —
        // every callee is `None`, so this seeds nothing: an additive precision that
        // only fires when the analysis is given the real classification.
        HirKind::Call { func, args, .. } => {
            use crate::primitives::def::RegionEffect;
            match call_effect(func, arena, call_class) {
                Some(RegionEffect::Stores { args: stored }) => {
                    // Store facet — the value goes into another arg / external
                    // structure (a containment edge), not across a frontier.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, other_seeds);
                        }
                    }
                }
                Some(RegionEffect::Sends { args: stored }) => {
                    // Fiber facet (send half) — `chan/send`'s message crosses to the
                    // receiving fiber, so it is a frontier crossing (fiber seeds +
                    // region-level fiber sites), distinct from a `Stores` containment.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, fiber_seeds);
                            record_frontier_sites(ctx, &a.expr, fiber_sites);
                        }
                    }
                }
                Some(RegionEffect::Mixed | RegionEffect::Unknown) => {
                    for a in args {
                        tail_sources(ctx, &a.expr, other_seeds);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    hir.for_each_child(|c| {
        collect_flow(
            ctx,
            c,
            call_class,
            container_contents,
            edges,
            return_seeds,
            fiber_seeds,
            other_seeds,
            return_sites,
            fiber_sites,
            lambda_captures,
        )
    });
}
