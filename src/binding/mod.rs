//! Binding resolution for lexical scope
//!
//! This module provides compile-time variable resolution, producing
//! VarRef values that tell the runtime exactly where to find each variable.
//!
//! # Architecture
//!
//! - `VarRef`: Enum representing a resolved variable (Local, Upvalue, Global)
//! - `ResolvedVar`: VarRef plus boxing information for mutable captures
//! - `Scope`: A single lexical scope level with bindings
//! - `ScopeStack`: Stack of scopes for nested lexical environments
//!
//! # Usage
//!
//! During AST construction:
//! 1. Push scopes when entering lambdas, let bindings, etc.
//! 2. Bind variables when they are defined
//! 3. Lookup variables when they are referenced
//! 4. Mark variables as captured/mutated when appropriate
//! 5. Pop scopes when leaving
//!
//! The resulting VarRef tells the compiler exactly which bytecode
//! instructions to emit for each variable access.

mod scope;
mod varref;

pub use scope::{Binding, Scope, ScopeStack};
pub use varref::{ResolvedVar, VarRef};
