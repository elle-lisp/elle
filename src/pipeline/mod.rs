//! Compilation pipeline: Syntax -> HIR -> LIR -> Bytecode
//!
//! This module provides the end-to-end compilation functions.

mod analyze;
mod cache;
mod compile;
mod eval;
mod fixpoint;
mod scan;

// Re-export public API
pub use analyze::{analyze, analyze_all, analyze_file};
pub use compile::{compile, compile_all, compile_file};
pub use eval::{eval, eval_all, eval_file, eval_syntax};

/// Compilation result
#[derive(Debug)]
pub struct CompileResult {
    pub bytecode: crate::compiler::Bytecode,
    pub warnings: Vec<String>,
}

/// Analysis-only result (no bytecode generation)
/// Used by linter and LSP which need HIR but not bytecode
pub struct AnalyzeResult {
    pub hir: crate::hir::Hir,
}
