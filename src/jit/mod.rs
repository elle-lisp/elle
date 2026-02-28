//! JIT compilation for Elle
//!
//! This module provides JIT compilation of pure LIR functions to native code
//! using Cranelift. Only `Effect::none()` functions are JIT candidates.
//!
//! ## Architecture
//!
//! ```text
//! LirFunction -> JitCompiler -> Cranelift IR -> Native x86_64 code -> JitCode
//! ```
//!
//! ## Calling Convention
//!
//! JIT-compiled functions use this calling convention:
//!
//! ```ignore
//! type JitFn = unsafe extern "C" fn(
//!     env: *const Value,      // closure environment (captures array)
//!     args: *const Value,     // arguments array
//!     nargs: u32,             // number of arguments
//!     vm: *mut VM,            // pointer to VM (for globals, function calls)
//!     self_bits: u64,         // NaN-boxed closure bits (for self-tail-call detection)
//! ) -> Value;
//! ```
//!
//! The 5th parameter `self_bits` enables self-tail-call optimization: when a
//! function tail-calls itself, the JIT compares the callee against `self_bits`.
//! If equal, it updates the arg variables and jumps to the loop header instead
//! of calling `elle_jit_tail_call`. This turns self-recursive tail calls into
//! native loops.

mod code;
mod compiler;
pub(crate) mod dispatch;
mod fastpath;
mod group;
mod runtime;
mod translate;

pub use code::JitCode;
pub use compiler::{BatchMember, JitCompiler};
pub use dispatch::TAIL_CALL_SENTINEL;
pub(crate) use group::discover_compilation_group;

use std::fmt;

/// JIT compilation error
#[derive(Debug, Clone)]
pub enum JitError {
    /// Instruction not supported by JIT
    UnsupportedInstruction(String),
    /// Function is not pure (may yield)
    NotPure,
    /// Cranelift compilation failed
    CompilationFailed(String),
    /// Invalid LIR structure
    InvalidLir(String),
}

impl fmt::Display for JitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JitError::UnsupportedInstruction(name) => {
                write!(f, "JIT: unsupported instruction: {}", name)
            }
            JitError::NotPure => write!(f, "JIT: function is not pure (may yield)"),
            JitError::CompilationFailed(msg) => write!(f, "JIT compilation failed: {}", msg),
            JitError::InvalidLir(msg) => write!(f, "JIT: invalid LIR: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}
