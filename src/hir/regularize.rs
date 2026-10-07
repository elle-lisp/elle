// audited: 2026-10-06
//! The post-analysis HIR regularization sequence, in the one order every compile entry runs it.
//!
//! docs/impl/graft.md
//! docs/impl/dissolution.md
//!
//! Every compile entry calls `regularize` between analysis and region
//! inference. It turns the analyzed HIR into the functionalized, ANF-lifted
//! form that region inference, escape analysis and the lowerer consume.

use crate::config::GraftPolicy;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::Hir;
use crate::hir::typeinfer::graft::{close_lambdas, graft_calls, GraftRecord};
use crate::hir::typeinfer::{
    fuse_map_chains, infer_and_rewrite, prune_typeof_match_arms, TypeInfo,
};
use crate::symbol::SymbolTable;
use crate::value::Value;
use std::collections::HashMap;

/// Prune dead `(type-of x)` arms, close each lambda's body into a fragment,
/// fuse, graft, drop dead bindings, mark tail calls, functionalize, ANF-lift,
/// and run type inference, the fold and the intrinsic operand proofs. Returns
/// the inference result (`Err` is the operand proof obligation; see `typeinfer`).
///
/// `values` maps each binding the runtime supplies to its compile-time value;
/// the graft and fusion read another unit's function body off it. Under
/// `GraftPolicy::Off` the graft is skipped and every call stays a call.
///
/// **The order matters.**
///
/// - Dead-arm pruning runs *before* `functionalize`. A dead arm of a
///   `(match (type-of x) …)` can introduce prebound or captured bindings, such
///   as the `each` macro's `(def @cur seq)`. `functionalize` would hoist them
///   to the enclosing scope's cell layout, and removing the arm after that
///   orphans the hoisted cell.
/// - Every lambda closes into its fragment before fusion or the graft rewrites
///   a body, so the template carries the body as written, dead arms pruned.
/// - Fusion and the graft run before tail marking, which marks the calls they
///   mint. They also run before dead-binding elimination, which drops the
///   bindings they leave unused before the region solver could mint a region
///   for them.
/// - Functionalization rebuilds every `Var` node, so it reports where the
///   occurrences the graft recorded went.
pub(crate) fn regularize(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbols: &SymbolTable,
    values: &HashMap<Binding, Value>,
    graft: GraftPolicy,
) -> Result<TypeInfo, String> {
    prune_typeof_match_arms(hir, arena);
    let unit = close_lambdas(hir, arena);
    fuse_map_chains(hir, arena, &unit, values);
    let mut record = match graft {
        GraftPolicy::On => graft_calls(hir, arena, &unit, values),
        GraftPolicy::Off => GraftRecord::default(),
    };
    crate::hir::dead::eliminate_dead_bindings(hir, arena, symbols);
    crate::hir::tailcall::mark_tail_calls(hir);
    let tracked = record.tracked_nodes().collect();
    let moved = crate::hir::functionalize::functionalize_tracking(hir, arena, &tracked);
    record.remap(&moved);
    crate::hir::anf::anf_lift(hir, arena);
    infer_and_rewrite(hir, arena, &record)
}
