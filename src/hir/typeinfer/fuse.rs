//! HOF-chain loop fusion — the first closure dissolution (docs/impl/dissolution.md).
//!
//! At a call `(map f xs)` / `(filter p xs)` where `xs` is a statically-proven
//! immutable array and the lambda is a non-capturing single-parameter one written
//! at the call site, this rewrites the cross-unit stdlib dispatch to the
//! index-walk loop that op's own array arm runs (`src/stdlib.lisp`) — but with the
//! lambda body **spliced inline** rather than called through a closure value. The
//! closure ceases to exist: no per-element closure allocation, no indirect call.
//! `map` pushes each transform's result; `map-indexed` does the same with the walk's
//! induction variable bound beside the element (`(f i elem)`); `filter` pushes the
//! element itself under
//! an `if` guard; `take-while` pushes it under a guard whose rejecting side ends
//! the run, `drop-while` under the complementary flag, which its rejecting side
//! clears to open the rest of the pipeline, and `mapcat` pushes every element of the
//! array its function returns, walked by a second loop. A chain's optional outermost
//! **terminal** is a scalar op:
//! `fold`/`reduce` (`(fold f init xs)`, `f` called `(f acc elem)`) threads an
//! accumulator seeded by `init` one left-fold step per element, `count`
//! (`(count pred xs)`) tallies the elements its predicate admits, and each of the
//! four short-circuiting searches `any?`/`all?`/`find`/`find-index` writes the
//! answer its first deciding element settles and clears the sentinel its loop
//! condition reads, so no later element is fetched — so there is no
//! `@array` and no `freeze`, and the result is the accumulator's final value. A
//! composition —
//! `(map g (map f xs))`, `(filter q (filter p xs))`, any mix like `(map f (filter
//! p xs))`, or a terminal over a map/filter prefix like `(fold f init (map g xs))` —
//! fuses to a **single** loop through one unified transform/guard pipeline
//! (`build_loop`/`Build::element`): each `map`/`filter` op is a *stage* (a `map`
//! transforms the threaded value; a `filter` guards it), the stages nest in
//! application order, and the base case is the terminal (a `push` for a collect, a
//! fold step for a fold, an increment for a count). The intermediate array any inner
//! op would have allocated never exists. `map`-only and `filter`-only chains are just
//! the all-transform and all-guard ends of the collect pipeline; a fold reuses the
//! same stages with a scalar terminal — the map-reduce shape, no array at all — and a
//! count is that same shape with its predicate appended as the pipeline's last guard.
//! A search is a count's shape again, appending the guard whichever way round its
//! answer is decided, and a `take-while` is a guard whose rejecting side ends the
//! run. Both carry an early exit, and the rule for where it is read is the same:
//! the chain's INNERMOST op may end the walk — nothing runs before it, so no
//! per-element work goes unrun — and its sentinel is the loop condition's. Every
//! other early exit gates its own stage while the walk stays exhaustive. A
//! `drop-while` carries no early exit at all: its flag opens the pipeline instead of
//! closing the walk, so the loop condition stays the bare range test whatever the
//! chain around it looks like. A `map-indexed` carries none either, and needs no
//! survivor count for its position: every stage that renumbers is one that shortens
//! the walk, and the emptiness rule (`Hof::preserves_length`) already refuses each
//! one inner to an untyped array arm, of which `map-indexed` is one. A `mapcat`
//! threads a whole RUN of values on where every other stage threads exactly one: its
//! element statement carries a SECOND walk over the collection its function returns,
//! with the rest of the pipeline spliced inside it, so each stage outer to it runs
//! once per spliced element — which is what the flat collection the stdlib op builds
//! gives them.
//!
//! Every counter the emitted loop owns advances by the raw `%add` opcode
//! (`Build::advance`), never the stdlib `+`, whose rest-list and `letrec` walker
//! would re-mint per element the very closure this pass dissolves.
//!
//! ## Why this shape, here
//!
//! The pass emits *surface* HIR — plain `while`/`push`/`freeze` (plus `if` for
//! `filter`), the same shape the stdlib op's body has before functionalization —
//! and runs in `regularize` (`src/hir/regularize.rs`) **before** `functionalize`.
//! So every downstream pass consumes the fused loop exactly as it consumes the
//! op's own body: the `while` becomes a `loop`/`recur`, `push` monomorphizes to
//! `%push-array-mut` on the proven `@array` accumulator, region inference frees
//! the accumulator by subtree drop. The pass never hand-builds a `loop`/`recur`
//! or a capture cell.
//!
//! It mirrors the container-dispatch monomorphization (`monomorphize.rs`):
//! recognize a proven-type call across the compile-unit boundary (the callee is
//! `is_primitive` — a `bind_primitives` stdlib export — and named `map`/`map-indexed`/
//! `filter`/
//! `take-while`/`drop-while`/`mapcat`/`fold`/`reduce`/`count`/`any?`/`all?`/`find`/`find-index`; a user redefinition
//! shadows it with a non-primitive binding and is left alone) and collapse it to the
//! direct form the proof selects.
//!
//! ## Legality
//!
//! Fusion preserves the program's value. A single op also preserves the exact
//! per-element evaluation order (the loop applies the lambda left to right,
//! identically to the stdlib op), so it needs no purity gate. A **composition**
//! interleaves the per-element work (`f x0; g …; f x1; g …`) rather than running
//! all of the first op then all of the second — a reorder observable through two
//! channels, so each lambda in a chain of length ≥ 2 must have neither. It must be
//! free of **sequencing effects** (`reorder_safe`): no yield/I/O/emit/FFI/halt.
//! `SIG_ERROR` is permitted; see `reorder_safe`. And it must be **non-capturing**
//! (`captures_locals`): a captured binding is state two bodies can share with no
//! signal to gate it. A lone op interleaves nothing and is asked neither question,
//! which is why a capturing literal fuses there — its body is spliced AT the call
//! site, so its free variables are in scope with no rename. An early exit would go
//! further than reordering if it
//! cut the walk short with a stage inner to it — leaving that stage's work unrun on
//! every element past the decision — so only the chain's innermost op ends the
//! walk; the others stop their own stage while the walk stays exhaustive.
//!
//! A body may hold a raw call-position `%`-intrinsic only under the function's own
//! `(numeric!)` declaration. That declaration floors the parameters at Number —
//! the fact that discharges the intrinsic's operand contract — and it is recorded
//! on the parameter BINDINGS (`BindingInner::declared_numeric`), so it travels with
//! the parameter the splice turns into a loop local and the site proves in the loop
//! exactly as it did in the function. Fusion therefore never changes whether a
//! program compiles (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies").

use super::prune::concrete_init_keywords;
use super::unwrap_callee_binding;
use crate::hir::arena::{BindingArena, BindingScope};
use crate::hir::binding::{Binding, CaptureKind};
use crate::hir::expr::{CallArg, Hir, HirKind};
use crate::primitives::def::RetType;
use crate::signals::{Signal, SIG_ERROR};
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

// The pass in reading order: `Ops` resolves the stdlib bindings the fused loop
// is built from; `chain` recognizes a fusable HOF chain and validates it;
// `registry` and `clone` supply the function bodies to splice, from this unit
// and from earlier ones; `build` emits the loop.
mod build;
mod chain;
mod clone;
mod ops;
mod registry;

// Glob-imported so each submodule's own `use super::*` reaches the others:
// the five split parts form one pass and refer to each other freely.
use build::*;
use chain::*;
use clone::*;
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
    symbols: &SymbolTable,
    registry: &mut FnInlineRegistry,
) {
    let symbol_names = symbols.all_names();
    // The same-unit function templates: a `Var` naming a non-capturing lambda
    // (a top-level `defn` or a `let`/`def`-bound `fn`) inlines like a literal
    // (docs/impl/dissolution.md § "Named same-unit functions"). Built once over
    // the pre-rewrite tree; each use clones a fresh copy, so the map stays valid
    // as calls collapse.
    let mut templates: FxHashMap<Binding, FnTemplate> = FxHashMap::default();
    collect_inline_fns(hir, arena, &mut templates, &mut FxHashSet::default());
    // Record this unit's cross-unit-inlineable functions by NAME so later units can
    // reach them (docs/impl/dissolution.md § "Cross-unit named functions"). Done
    // BEFORE `Ops::resolve` below: during the `<stdlib>` compile the loop-scaffold
    // primitives are not yet `is_primitive`, so `Ops::resolve` fails and fusion is
    // inert here — but the stdlib is exactly where the `inc`/`dec` templates that
    // later units inline are defined, so the recording must not sit behind that gate.
    record_cross_unit_fns(hir, arena, registry);
    let Some(ops) = Ops::resolve(arena, &symbol_names) else {
        return;
    };
    // The sound `binding → type-of keyword` proof dead-arm pruning already
    // computes (`prune::concrete_init_keywords`). A `map`'s base collection may be
    // a `Var` alias of an immutable array, not only a call-site literal; this map
    // is what proves the alias `array`. Built once over the pre-rewrite tree — the
    // base-var bindings live in enclosing `let`s that fusion never mutates, so the
    // proof stays valid as inner map calls collapse.
    let bases = concrete_init_keywords(hir, arena, &symbol_names);
    // This unit's primitives by name, so a cross-unit template's free globals
    // (recorded by name in a different arena) re-resolve to this arena's bindings.
    // `bind_primitives` binds each primitive/stdlib-export once, so first-wins is
    // exact — the same map `monomorphize.rs` builds for the dispatch registry.
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
    rewrite(hir, arena, &symbol_names, &ops, &bases, &fns);
}

#[cfg(test)]
mod tests;
