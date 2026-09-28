// audited: 2026-09-28
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
//! the pass runs in `regularize` before `functionalize`, so every later pass
//! lowers it exactly as it lowers the stdlib op's own body. The pass never builds
//! a `loop`/`recur` or a capture cell by hand.
//!
//! A function argument is a call-site lambda literal, which the rewrite moves and
//! which may capture when its op is alone in the chain (`captures_locals`), or a
//! named function whose body a fragment carries: this unit's, by binding, or an
//! earlier unit's, by name through the registry. A chain of two or more ops
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
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

// The pass in reading order: `Ops` resolves the stdlib bindings the fused loop
// is built from; `chain` recognizes a fusable HOF chain and validates it;
// `collect` closes this unit's inlineable function bodies into fragments and
// `registry` holds the ones earlier units left; `build` emits the loop.
mod build;
mod chain;
mod collect;
mod ops;
mod registry;

// Glob-imported so each submodule's own `use super::*` reaches the others:
// the five split parts form one pass and refer to each other freely.
use build::*;
use chain::*;
use collect::*;
use ops::*;
use registry::*;

pub(crate) use registry::{FnInlineRegistry, StoredFnInlineRegistry};

/// Fuse every qualifying HOF chain into an inlined index-walk loop. Runs on
/// surface HIR, before functionalize (see the module doc). `registry` is the
/// per-instance cross-unit function-inline registry: this unit's inlineable
/// functions are recorded into it (so later units can inline them) and its earlier
/// entries — the `<stdlib>` compile's `inc`/`dec`/… — are consulted here.
pub(crate) fn fuse_map_chains(
    hir: &mut Hir,
    arena: &mut BindingArena,
    registry: &mut FnInlineRegistry,
) {
    // The sound `binding → type-of keyword` proof dead-arm pruning already
    // computes (`prune::concrete_init_keywords`). A `map`'s base collection may be
    // a `Var` alias of an immutable array, not only a call-site literal; this map
    // is what proves the alias `array`. Built once over the pre-rewrite tree — the
    // base-var bindings live in enclosing `let`s that fusion never mutates, so the
    // proof stays valid as inner map calls collapse. A collected fragment reads it
    // too, for the array proof a `mapcat` asks of its function.
    let bases = concrete_init_keywords(hir, arena);
    // Close this unit's inlineable functions into fragments: by `Binding` for a
    // `Var` naming one here, and by NAME into the registry so later units can
    // reach them (docs/impl/dissolution/inline.md). Done over the pre-rewrite
    // tree, and BEFORE `Ops::resolve` below: during the `<stdlib>` compile the
    // loop-scaffold primitives are not yet `is_primitive`, so `Ops::resolve` fails
    // and fusion is inert here — but the stdlib is exactly where the `inc`/`dec`
    // fragments that later units inline are defined, so the recording must not
    // sit behind that gate.
    let mut collector = Collector::new(arena, &bases);
    collector.walk(hir, registry);
    let templates = collector.templates;
    let Some(ops) = Ops::resolve(arena) else {
        return;
    };
    // This unit's primitives by name, so a fragment's free globals — recorded by
    // name, since a `Binding` means nothing outside the arena that minted it —
    // resolve to this arena's bindings. `bind_primitives` binds each
    // primitive/stdlib-export once, so first-wins is exact, and the map is the
    // same one `monomorphize.rs` builds for the dispatch registry.
    let mut prim_by_name: FxHashMap<SymbolId, Binding> = FxHashMap::default();
    for i in 0..arena.len() as u32 {
        let b = Binding(i);
        let bi = arena.get(b);
        if bi.is_primitive {
            prim_by_name.entry(bi.name).or_insert(b);
        }
    }
    let fns = FnResolver {
        templates: &templates,
        registry,
        prim_by_name: &prim_by_name,
    };
    rewrite(hir, arena, &ops, &bases, &fns);
}

/// Pre-order walk: try to fuse a HOF chain rooted at `hir` (consuming the whole
/// chain, including its inner HOF calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested HOFs in the spliced lambda
/// bodies or the base array's elements). A chain of pipeline stages under
/// an optional outermost scalar terminal, over the same proven base, fuses to one
/// loop; the recursion still reaches HOFs nested inside a spliced lambda body or a
/// declined chain's inner run (a chain declined by the reorder gate, say, whose
/// inner run then fuses on its own).
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
