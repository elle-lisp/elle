// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The inference pass's unit tests, and the helpers they compile through and
//! read the result with.
//!
//! One module per subject: `narrow` for what a guard or a dispatch arm proves,
//! `rettype` for what an op's result proves, `recursion` for what a recursive
//! call's result proves, `mutation` for what a written binding proves,
//! `bottom` for what the lattice's start proves, `prune` for dead-arm removal,
//! `contract` for prove-or-reject, `graft` for which calls become their
//! callee's body, `fold` for what the recorded types then delete.

use super::infer_and_rewrite;
use crate::hir::types::TyId;
use crate::hir::{BindingArena, Hir, HirId, HirKind};
use crate::symbol::SymbolTable;

mod bottom;
mod contract;
mod fold;
mod graft;
mod mutation;
mod narrow;
mod prune;
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

/// The set of inferred node types for `src`.
fn inferred_types(src: &str) -> Vec<TyId> {
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    let info = infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
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
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    let info = infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
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

/// Count Call nodes whose callee resolves to a binding named `name`.
fn count_calls_to(hir: &Hir, arena: &BindingArena, name: &str) -> usize {
    let mut n = 0;
    if let HirKind::Call { func, .. } = &hir.kind {
        if let Some(b) = super::unwrap_callee_binding(func) {
            if arena.get(b).name == crate::value::SymbolId::of(name) {
                n += 1;
            }
        }
    }
    hir.for_each_child(|c| n += count_calls_to(c, arena, name));
    n
}

/// Compile `src` through the file front end (which runs `infer_and_rewrite`),
/// discarding the result and surfacing only success/failure. `Err` is the
/// monomorphization proof obligation firing (or any earlier compile error).
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
