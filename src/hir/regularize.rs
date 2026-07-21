//! The post-analysis HIR regularization sequence.
//!
//! Between analysis and region inference, every compile entry transforms the
//! analyzed HIR into the canonical (functionalized + ANF) form the region/escape
//! analyses and the lowerer consume. Centralizing the sequence here keeps its
//! order — which is load-bearing — identical at every call site.

use crate::hir::arena::BindingArena;
use crate::hir::expr::Hir;
use crate::hir::typeinfer::{
    infer_and_rewrite, prune_typeof_match_arms, DispatchWrapperRegistry, TypeInfo,
};
use crate::symbol::SymbolTable;

/// Mark tail calls, prune dead `(type-of x)` arms, functionalize, ANF-lift, and
/// run type inference and the intrinsic operand proofs. Returns the inference result (`Err`
/// is the monomorphization proof obligation; see `typeinfer`).
///
/// **Order is load-bearing.** Dead-arm pruning runs *before* `functionalize`: a
/// `(match (type-of x) …)` dead arm can introduce prebound/captured bindings (the
/// `each` macro's `(def @cur seq)`) that `functionalize` would hoist to the
/// enclosing scope's cell layout — after which removing the arm orphans the
/// hoisted cell and leaves a reference the lowerer cannot resolve. Pruning first
/// means functionalize never sees the dead arm. Pruning also runs before escape
/// analysis sees the tree, so the off-type arms no longer mark the scrutinee's
/// region escaping (the io-yield / `each`-over-collection leak; `typeinfer/prune.rs`).
pub(crate) fn regularize(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbols: &SymbolTable,
    dispatch_wrappers: &mut DispatchWrapperRegistry,
) -> Result<TypeInfo, String> {
    prune_typeof_match_arms(hir, arena, symbols);
    crate::hir::tailcall::mark_tail_calls(hir);
    crate::hir::functionalize::functionalize(hir, arena);
    crate::hir::anf::anf_lift(hir, arena);
    infer_and_rewrite(hir, arena, symbols, dispatch_wrappers)
}
