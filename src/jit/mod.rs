//! JIT compilation for Elle
//!
//! This module provides JIT compilation of pure LIR functions to native code
//! using Cranelift. Only `Effect::Pure` functions are JIT candidates.
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
//!     globals: *mut (),       // pointer to VM globals
//! ) -> Value;
//! ```

#[cfg(feature = "jit")]
mod code;
#[cfg(feature = "jit")]
mod compiler;
#[cfg(feature = "jit")]
mod runtime;

#[cfg(feature = "jit")]
pub use code::JitCode;
#[cfg(feature = "jit")]
pub use compiler::JitCompiler;

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
