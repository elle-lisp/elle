// audited: 2026-10-22
//! The compilation pipeline's entry points: source text to bytecode, or to HIR
//! for a reader that wants the analysis alone.
//!
//! src/pipeline/AGENTS.md
//!
//! Each stage lives beside this file:
//!
//! - `cache`: the compile-context type an instance threads through every call.
//! - `bootstrap`: the core.lisp bootstrap that builds one, and the exports a
//!   boot leaves.
//! - `sources`: the three sources a boot compiles.
//! - `compile`, `analyze` and `eval`: the three surfaces over them.

mod analyze;
mod bootstrap;
mod cache;
mod compile;
pub mod directives;
mod eval;
pub mod sources;

// Re-export public API
pub use analyze::{analyze, analyze_file};
pub use bootstrap::{install_core_exports, BootExports};
pub use cache::{compile_file_stored, CompileCtx, Layer};
#[cfg(test)]
pub(crate) use compile::compile_file_to_typed_fhir;
pub use compile::{
    compile, compile_barrier_module, compile_file, compile_file_repl, compile_file_to_fhir,
    compile_file_to_lir, compile_value, compile_whole_module, compile_whole_module_forms,
    splice_includes,
};
pub(crate) use compile::{IncludeName, IncludeRead, Includes};
pub use eval::{eval, eval_all, eval_file, eval_syntax};

/// Analysis-only result (no bytecode generation), for the linter, the LSP,
/// `compile/analyze` and the signal solver, which need HIR but not bytecode.
#[derive(Debug)]
pub struct AnalyzeResult {
    /// The analyzed tree, with tail calls already marked. Consumers that read
    /// `is_tail` off an analysis — the linter's non-tail-self-recursion rule, the
    /// `compile/callees` call-graph builder — read a flag the analysis set, not a
    /// default. `regularize` marks again on the compile path, because map fusion
    /// and the graft mint call nodes after this point.
    pub hir: crate::hir::Hir,
    pub arena: crate::hir::BindingArena,
    /// Accumulated non-fatal analysis errors
    pub errors: Vec<crate::error::LError>,
    /// Each lambda's declared ceiling and the bits it raises by itself, keyed
    /// by its node.
    pub lambda_decls: std::collections::HashMap<crate::hir::HirId, crate::hir::LambdaDecl>,
    /// The bits the code outside every lambda raises by itself.
    pub top_direct: crate::value::fiber::SignalBits,
}
