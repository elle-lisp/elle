// audited: 2026-10-13
//! The post-analysis HIR regularization sequence, in the one order every compile entry runs it.
//!
//! docs/impl/hir.md
//!
//! Between analysis and region inference, every compile entry transforms the
//! analyzed HIR into the canonical (functionalized + ANF) form the region/escape
//! analyses and the lowerer consume. The sequence lives here so that its order
//! is the same at every call site.

use crate::hir::arena::BindingArena;
use crate::hir::expr::Hir;
use crate::hir::typeinfer::{
    fuse_map_chains, infer_and_rewrite, prune_typeof_match_arms, Registries, TypeInfo,
};
use crate::symbol::SymbolTable;

/// Prune dead `(type-of x)` arms, fuse HOF chains, eliminate dead bindings, mark
/// tail calls, functionalize, ANF-lift, and run type inference and the intrinsic
/// operand proofs. Returns the inference result (`Err` is the intrinsic operand
/// proof obligation; see `typeinfer`). `registries` are
/// the instance's cross-unit registries as this compile sees them
/// (docs/impl/dissolution/inline.md).
///
/// **The order matters.** Dead-arm pruning runs *before* `functionalize`: a
/// `(match (type-of x) …)` dead arm can introduce prebound/captured bindings (the
/// `each` macro's `(def @cur seq)`) that `functionalize` would hoist to the
/// enclosing scope's cell layout — after which removing the arm orphans the
/// hoisted cell and leaves a reference the lowerer cannot resolve. Pruning first
/// means functionalize never sees the dead arm. Pruning also runs before escape
/// analysis sees the tree, so the off-type arms no longer mark the scrutinee's
/// region escaping (the `each`-over-collection over-keep; `typeinfer/prune.rs`).
pub(crate) fn regularize(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbols: &SymbolTable,
    registries: &mut Registries,
) -> Result<TypeInfo, String> {
    prune_typeof_match_arms(hir, arena);
    // Closure dissolution: fuse `(map f xs)` / `(map g (map f xs))` over a proven
    // immutable array into an inlined index-walk loop, before functionalize sees
    // the tree so the loop lowers exactly as `map`'s own body does
    // (docs/impl/dissolution.md). Runs after pruning (which leaves map calls
    // untouched) and before tail-call marking, so the fused loop's `freeze` tail
    // is marked in place of the collapsed `map` call.
    fuse_map_chains(hir, arena, registries);
    // Dead binding elimination runs after pruning and fusion, which both make
    // bindings unused, and before functionalize, for the same reason pruning
    // does: the region solver runs after these transforms, so a call deleted
    // here mints no region and strands no release obligation.
    crate::hir::dead::eliminate_dead_bindings(hir, arena, symbols);
    crate::hir::tailcall::mark_tail_calls(hir);
    crate::hir::functionalize::functionalize(hir, arena);
    crate::hir::anf::anf_lift(hir, arena);
    infer_and_rewrite(hir, arena, registries)
}
