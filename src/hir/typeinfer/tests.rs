// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/impl/hir.md
//! The inference pass's unit tests, and the three helpers every one of them
//! compiles through.
//!
//! One module per subject: `narrow` for what a guard or a dispatch arm proves,
//! `rettype` for what an op's result proves, `prune` for dead-arm removal,
//! `contract` for prove-or-reject, `monomorphize` for wrapper collapse.

use super::infer_and_rewrite;
use crate::hir::types::TyId;
use crate::hir::{BindingArena, Hir};
use crate::symbol::SymbolTable;

mod contract;
mod monomorphize;
mod narrow;
mod prune;
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

/// Compile `src` through the file front end (which runs `infer_and_rewrite`),
/// discarding the result and surfacing only success/failure. `Err` is the
/// monomorphization proof obligation firing (or any earlier compile error).
fn compile_result(src: &str) -> Result<(), String> {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(src, &mut symbols, &mut cctx, "<test>").map(|_| ())
}
