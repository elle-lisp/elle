//! Language Server Protocol implementation for Elle Lisp
//!
//! A resident compiler-based LSP server that provides:
//! - Real-time diagnostics from integrated linter
//! - Hover information with symbol lookup
//! - Code completion suggestions
//! - Navigation to symbols

pub mod compiler_state;
pub mod completion;
pub mod definition;
pub mod formatting;
pub mod handler;
pub mod hover;
pub mod protocol;
pub mod references;

pub use compiler_state::CompilerState;
