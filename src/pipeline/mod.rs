//! Compilation pipeline: Syntax -> HIR -> LIR -> Bytecode
//!
//! This module provides the end-to-end compilation functions.

mod analyze;
mod cache;
mod compile;
mod eval;

// Re-export public API
pub use analyze::{analyze, analyze_file};
pub use cache::CompileCtx;
pub use compile::{
    compile, compile_barrier_module, compile_file, compile_file_repl, compile_file_to_fhir,
    compile_file_to_lir, compile_whole_module, compile_whole_module_forms, splice_includes,
};
pub use eval::{eval, eval_all, eval_file, eval_syntax};

/// Compilation result
#[derive(Debug)]
pub struct CompileResult {
    pub bytecode: crate::compiler::Bytecode,
}

/// Analysis-only result (no bytecode generation)
/// Used by linter and LSP which need HIR but not bytecode
#[derive(Debug)]
pub struct AnalyzeResult {
    /// The analyzed tree, with tail calls already marked. Consumers that read
    /// `is_tail` off an analysis — the linter's non-tail-self-recursion rule, the
    /// `compile/callees` call-graph builder — read a flag the analysis set, not a
    /// default. `regularize` marks again on the compile path, because map fusion
    /// mints call nodes after this point.
    pub hir: crate::hir::Hir,
    pub arena: crate::hir::BindingArena,
    /// Accumulated non-fatal analysis errors
    pub errors: Vec<crate::error::LError>,
}
