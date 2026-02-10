//! Resident compiler for caching and managing compiled expressions
//!
//! Provides a shared compilation interface used by both LSP and CLI to:
//! - Cache compiled expressions (disk-backed in /dev/shm and in-memory)
//! - Maintain symbol tables and VM state
//! - Track source locations for error reporting
//! - Support both file-based and text-based compilation

pub mod cache;
pub mod compiled_doc;
pub mod compiler;

pub use compiled_doc::CompiledDocument;
pub use compiler::ResidentCompiler;
