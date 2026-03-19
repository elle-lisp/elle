//! JIT compilation for Elle
//!
//! This module provides JIT compilation of LIR functions to native code
//! using Cranelift. Functions with `Signal::silent()` or `Signal::yields()` are
//! JIT candidates. Polymorphic functions remain excluded.
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

mod calls;
mod code;
mod compiler;
mod data;
pub(crate) mod dispatch;
mod fastpath;
mod group;
mod helpers;
mod runtime;
mod suspend;
mod translate;
mod value;
mod vtable;

pub use code::JitCode;
pub use compiler::{BatchMember, JitCompiler};
pub use dispatch::{TAIL_CALL_SENTINEL, YIELD_SENTINEL};
pub(crate) use group::discover_compilation_group;
pub use value::JitValue;

use std::fmt;

/// JIT compilation error
#[derive(Debug, Clone)]
pub enum JitError {
    /// Instruction not supported by JIT
    UnsupportedInstruction(String),
    /// Function has polymorphic signal
    Polymorphic,
    /// Function has yielding signal (rejected by batch compilation only)
    Yielding,
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
            JitError::Polymorphic => write!(f, "JIT: function has polymorphic signal"),
            JitError::Yielding => write!(f, "JIT: yielding functions cannot be batch-compiled"),
            JitError::CompilationFailed(msg) => write!(f, "JIT compilation failed: {}", msg),
            JitError::InvalidLir(msg) => write!(f, "JIT: invalid LIR: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}

/// Record of a closure that was rejected from JIT compilation.
/// One entry per closure template, deduplicated by bytecode pointer.
#[derive(Debug, Clone)]
pub struct JitRejectionInfo {
    /// Function name (from `LirFunction.name`), if available.
    pub name: Option<String>,
    /// Why the JIT rejected this closure.
    pub reason: JitError,
}
