// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The inference pass's unit tests, and the four helpers every one of them
//! compiles through.
//!
//! One module per subject: `narrow` for what a guard or a dispatch arm proves,
//! `rettype` for what an op's result proves, `recursion` for what a recursive
//! call's result proves, `mutation` for what a call to a written binding
//! proves, `bottom` for what the lattice's start proves, `prune` for dead-arm
//! removal, `contract` for prove-or-reject, `monomorphize` for wrapper
//! collapse.

use super::infer_and_rewrite;
use crate::hir::types::TyId;
use crate::hir::{BindingArena, Hir, HirId, HirKind};
use crate::symbol::SymbolTable;

mod bottom;
mod contract;
mod monomorphize;
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

/// Compile `src` through the file front end (which runs `infer_and_rewrite`),
/// discarding the result and surfacing only success/failure. `Err` is the
/// monomorphization proof obligation firing (or any earlier compile error).
fn compile_result(src: &str) -> Result<(), String> {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(src, &mut symbols, &mut cctx, "<test>").map(|_| ())
}
