// audited: 2026-10-28
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The inference pass's unit tests, and the helpers they compile through and
//! read the result with.
//!
//! One module per subject: `narrow` for what a guard or a dispatch arm proves,
//! `rettype` for what an op's result proves, `recursion` for what a recursive
//! call's result proves, `mutation` for what a written binding proves,
//! `bottom` for what the lattice's start proves, `callsite` for what a call
//! contributes where its operands and parameters do not line up,
//! `destructure` for what a destructuring write proves, `branch` for what a
//! branch's write and a block's `break` leave behind, `prune` for dead-arm
//! removal, `contract` for prove-or-reject, `graft` for which calls become
//! their callee's body, `operand` for which operands a graft writes in place,
//! `live` for which lambdas the graft reaches, `fold` for what the recorded
//! types then delete, `record` for what a grafted call gives its definition
//! across the fold's rounds.

use super::TypeInfo;
use crate::hir::types::TyId;
use crate::hir::{BindingArena, Hir, HirId, HirKind};
use crate::symbol::SymbolTable;

mod bottom;
mod branch;
mod callsite;
mod contract;
mod destructure;
mod fold;
mod graft;
mod live;
mod mutation;
mod narrow;
mod operand;
mod prune;
mod record;
mod recursion;
mod rettype;

/// Compile a source file to canonical (functionalized) HIR, mirroring the
/// escape-test harness. `infer_and_rewrite` runs unconditionally on every
/// compile (driven from `hir::regularize`).
fn compile_fhir(src: &str, symbols: &mut SymbolTable) -> (Hir, BindingArena) {
    let mut cctx = crate::pipeline::CompileCtx::new();
    let (hir, arena) =
        crate::pipeline::compile_file_to_fhir(src, symbols, &mut cctx, "<test>").expect("compile");
    (hir, arena)
}

/// Compile `src` as `compile_fhir` does, with the types the compile itself
/// inferred. Running inference again over the result would lose what the graft
/// handed the compile's inference, so a test reads these.
fn compile_typed(src: &str) -> (Hir, BindingArena, TypeInfo) {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    let (hir, arena, info) =
        crate::pipeline::compile_file_to_typed_fhir(src, &mut symbols, &mut cctx, "<test>")
            .expect("compile");
    (hir, arena, info)
}

/// The set of inferred node types for `src`.
fn inferred_types(src: &str) -> Vec<TyId> {
    let (_hir, _arena, info) = compile_typed(src);
    info.hir_types.values().copied().collect()
}

/// The inferred type of every `%`-intrinsic node named `op` in `src`, in the
/// order the walk reaches them.
///
/// Sharper than `inferred_types`, which reports every node's type at once: over
/// int literals the operands put `Int` into that set themselves, so an op's own
/// result rule can only be read off the op's own node.
fn intrinsic_result_types(src: &str, op: &str) -> Vec<TyId> {
    fn find(h: &Hir, op: &str, found: &mut Vec<HirId>) {
        if let HirKind::Intrinsic { op: this, .. } = &h.kind {
            if this.name() == op {
                found.push(h.id);
            }
        }
        h.for_each_child(|c| find(c, op, found));
    }
    let (hir, _arena, info) = compile_typed(src);
    let mut found = Vec::new();
    find(&hir, op, &mut found);
    assert!(!found.is_empty(), "{src} compiled to no {op} node");
    found
        .into_iter()
        .map(|id| {
            info.hir_types
                .get(&id)
                .copied()
                .unwrap_or_else(|| panic!("an {op} node in {src} carries no inferred type"))
        })
        .collect()
}

/// The inferred type of the one `%`-intrinsic node named `op` in `src`.
fn intrinsic_result_type(src: &str, op: &str) -> TyId {
    let found = intrinsic_result_types(src, op);
    assert_eq!(found.len(), 1, "{src} holds more than one {op} node");
    found[0]
}

use crate::hir::testkit::count_calls_to;

/// Compile `src` through the file front end (which runs `infer_and_rewrite`),
/// discarding the result and surfacing only success/failure. `Err` is the
/// intrinsic operand proof obligation firing (or any earlier compile error).
fn compile_result(src: &str) -> Result<(), String> {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(src, &mut symbols, &mut cctx, "<test>").map(|_| ())
}

/// Compile `src` through the file front end of a runtime with the stdlib
/// loaded, returning the regularized HIR. The stdlib is what gives a source's
/// `+` or `push` a body to graft; without it those names name nothing.
fn compile_booted(src: &str) -> Result<(Hir, BindingArena), String> {
    let mut rt = crate::runtime::Runtime::new();
    let (_vm, symbols, cctx) = rt.parts();
    crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>")
}

/// [`compile_booted`], for a source the test expects to compile.
fn compiled(src: &str) -> (Hir, BindingArena) {
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"))
}

/// Compile and run `src` on a runtime with the stdlib loaded, answering the
/// value of its last form. A test that asks what a program computes, rather
/// than what its tree holds, runs it here.
fn run_booted(src: &str) -> crate::value::Value {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::eval_all(src, symbols, vm, cctx, "<test>")
        .unwrap_or_else(|e| panic!("{src} must run: {e}"))
}

/// How many `%`-intrinsic nodes named `op` the tree holds.
fn count_intrinsics(h: &Hir, op: &str) -> usize {
    let mut n =
        usize::from(matches!(&h.kind, HirKind::Intrinsic { op: this, .. } if this.name() == op));
    h.for_each_child(|c| n += count_intrinsics(c, op));
    n
}

/// How many sites run the op `name`, in either spelling: a call to the
/// primitive binding of that name, or an intrinsic node of it. A stdlib body
/// spells its ops as intrinsics, and a rewrite may leave a call to the
/// primitive; a test asks whether the op runs, not which spelling it has.
fn count_ops(hir: &Hir, arena: &BindingArena, name: &str) -> usize {
    count_calls_to(hir, arena, name) + count_intrinsics(hir, name)
}

/// How many `emit` nodes the tree holds — every guard a grafted wrapper brings
/// raises through one, so none left means no guard is left.
fn count_emits(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::Emit { .. }));
    h.for_each_child(|c| n += count_emits(c));
    n
}

/// How many calls to `name` the tree holds with a spliced operand, and how many
/// without one. A spliced call never grafts, and a definition keeps the one its
/// general arm makes through `apply`, so a test about a call counts the
/// unspliced ones.
fn calls_by_splice(h: &Hir, arena: &BindingArena, name: &str) -> (usize, usize) {
    let mut counts = (0, 0);
    if let HirKind::Call { func, args, .. } = &h.kind {
        if let Some(b) = crate::hir::typeinfer::unwrap_callee_binding(func) {
            if arena.get(b).name == crate::value::SymbolId::of(name) {
                if args.iter().any(|a| a.spliced) {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
            }
        }
    }
    h.for_each_child(|c| {
        let (s, w) = calls_by_splice(c, arena, name);
        counts.0 += s;
        counts.1 += w;
    });
    counts
}
