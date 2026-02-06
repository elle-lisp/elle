//! Language Server Protocol implementation for Elle Lisp
//!
//! A simplified LSP server that provides:
//! - Real-time diagnostics from elle-lint
//! - Basic hover information for functions
//! - Symbol definitions and references

pub mod handler;
pub mod protocol;
