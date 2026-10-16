// audited: 2026-10-14
//! HOF-chain loop fusion: a higher-order call over a proven array becomes one loop
//! with its function's body spliced in.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/inline.md
//!
//! A chain of pipeline stages (`map`, `map-indexed`, `filter`, `take-while`,
//! `drop-while`, `mapcat`) under an optional scalar terminal (`fold`/`reduce`,
//! `count`, `any?`, `all?`, `find`, `find-index`), over one proven base, fuses to
//! a single loop. The loop is *surface* HIR — plain `while`/`push`/`freeze` — and
//! the pass runs in `regularize` before the graft and `functionalize`, so every later pass
//! lowers it exactly as it lowers the stdlib op's own body. The pass never builds
//! a `loop`/`recur` or a capture cell by hand.
//!
//! A function argument is a call-site lambda literal, which the rewrite moves and
//! which may capture when its op is alone in the chain (`captures_locals`), or a
//! named function whose body a fragment carries: this unit's, by binding, or
//! another unit's, on its function value's code payload. A chain of two or more ops
//! interleaves its functions' calls, so each must pass `reorder_safe` and capture
//! nothing. Only the chain's innermost op may end the walk early; every other
//! early exit gates its own stage.
//!
//! A body may hold a call-position `%`-intrinsic only under its function's
//! `(numeric!)` declaration, which floors the parameter BINDINGS
//! (`BindingInner::declared_numeric`). The floor travels with the parameter the
//! splice turns into a loop local, so fusion never changes whether a program
//! compiles.

use super::prune::concrete_init_keywords;
use super::unwrap_callee_binding;
use crate::hir::arena::{BindingArena, BindingScope};
use crate::hir::binding::{Binding, CaptureKind};
use crate::hir::expr::{CallArg, Hir, HirKind};
use crate::primitives::def::RetType;
use crate::signals::{Signal, SIG_ERROR};
use crate::value::SymbolId;
use rustc_hash::FxHashMap;
use std::collections::HashMap;

// The pass in reading order: `Ops` resolves the bindings the fused loop calls;
// `chain` recognizes a fusable HOF chain and validates it; `resolve` finds the
// body a function argument names; `build` emits the loop.
mod build;
mod chain;
mod ops;
mod resolve;

// Glob-imported so each submodule's own `use super::*` reaches the others:
// the four split parts form one pass and refer to each other freely.
use build::*;
use chain::*;
use ops::*;
use resolve::*;

pub(in crate::hir::typeinfer) use chain::classify_base;

/// Fuse every qualifying HOF chain into an inlined index-walk loop. A function
/// argument that names a function reaches its body through `unit`, this unit's
/// closed functions, or through the fragment on the code payload of the value
/// `values` holds for a binding the runtime supplies, which `memo` decodes once
/// per compile.
pub(crate) fn fuse_map_chains(
    hir: &mut Hir,
    arena: &mut BindingArena,
    unit: &super::graft::UnitFns,
    values: &HashMap<Binding, crate::value::Value>,
    memo: &super::graft::FragmentMemo,
) {
    // The `binding → type-of keyword` proof dead-arm pruning computes. A base
    // collection may be a `Var` alias of a proven array, not only a call-site
    // literal, and this map is what proves the alias. It is built once over the
    // pre-rewrite tree: the base bindings live in enclosing `let`s that fusion
    // never mutates, so the proof stays valid as inner chains collapse.
    let bases = concrete_init_keywords(hir, arena);
    let Some(ops) = Ops::resolve(arena) else {
        return;
    };
    let prim_by_name = primitives_by_name(arena);
    let fns = FnResolver {
        unit,
        values,
        memo,
        prim_by_name: &prim_by_name,
    };
    rewrite(hir, arena, &ops, &bases, &fns);
}

/// This unit's primitives by name, so a fragment's free globals resolve to
/// this arena's bindings. A fragment records a global by name, because a
/// `Binding` means nothing outside the arena that minted it. `bind_primitives`
/// binds each primitive and stdlib export once, so the first binding found is
/// the only one.
pub(in crate::hir::typeinfer) fn primitives_by_name(
    arena: &BindingArena,
) -> FxHashMap<SymbolId, Binding> {
    let mut prim_by_name: FxHashMap<SymbolId, Binding> = FxHashMap::default();
    for i in 0..arena.len() as u32 {
        let b = Binding(i);
        let bi = arena.get(b);
        if bi.is_primitive {
            prim_by_name.entry(bi.name).or_insert(b);
        }
    }
    prim_by_name
}

/// Pre-order walk: fuse the chain rooted at `hir` when the gate admits it,
/// consuming its inner op calls, then recurse into the children of whatever
/// node stands there. The recursion reaches an op inside a spliced body or a
/// base expression, and the inner run of a chain the gate declined.
fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    ops: &Ops,
    bases: &FxHashMap<Binding, &'static str>,
    fns: &FnResolver,
) {
    if let Some(plan) = validate_chain(hir, arena, bases, fns) {
        let sig = hir.signal;
        let span = hir.span;
        let owned = std::mem::replace(hir, Hir::error(span));
        let chain = take_chain(owned, plan, arena, fns);
        *hir = build_loop(chain, arena, ops, sig, span);
    }
    hir.for_each_child_mut(|c| rewrite(c, arena, ops, bases, fns));
}

#[cfg(test)]
mod tests;
