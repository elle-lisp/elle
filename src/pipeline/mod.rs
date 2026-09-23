// audited: 2026-09-21
//! The compilation pipeline's entry points: source text to bytecode, or to HIR
//! for a reader that wants the analysis alone.
//!
//! src/pipeline/AGENTS.md
//!
//! Each stage lives beside this file: the compile-context type an instance
//! threads through every call (`cache`), the core.lisp bootstrap that builds
//! one (`bootstrap`), the three sources a boot compiles (`sources`), and the
//! three surfaces over them — `compile`, `analyze` and `eval`.

mod analyze;
mod bootstrap;
mod cache;
mod compile;
pub mod directives;
mod eval;
pub mod sources;

// Re-export public API
pub use analyze::{analyze, analyze_file};
pub use bootstrap::install_core_exports;
pub use cache::{BootExports, CompileCtx};
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
