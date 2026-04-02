//! Compilation pipeline: Syntax -> HIR -> LIR -> Bytecode
//!
//! This module provides the end-to-end compilation functions.

mod analyze;
mod cache;
mod compile;
mod eval;

// Re-export public API
pub use analyze::{analyze, analyze_file};
pub use cache::{
    lookup_stdlib_value, register_repl_binding, register_repl_macros, update_cache_with_stdlib,
};
pub use compile::{
    compile, compile_file, compile_file_repl, compile_file_to_lir, compile_to_lir, splice_includes,
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
    pub hir: crate::hir::Hir,
    pub arena: crate::hir::BindingArena,
}
