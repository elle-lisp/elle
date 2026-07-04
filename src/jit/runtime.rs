//! Runtime helper functions for JIT-compiled code
//!
//! These functions are called from JIT-compiled code to perform operations
//! that are too complex to inline, such as arithmetic with type checking.
//!
//! All functions use the C calling convention and operate on (tag, payload)
//! pairs representing 16-byte Values.
//!
//! `JitValue` with `#[repr(C)]` is FFI-compatible on all Cranelift targets:
//! a two-field struct of u64s is returned in a register pair (rax:rdx on
//! x86-64, x0:x1 on aarch64), matching Cranelift's two-I64 return convention.

use crate::jit::value::JitValue;
use crate::value::repr::TAG_INT;
use crate::value::repr::TAG_NIL;
use crate::value::Value;

/// Type error helper that takes a static string
fn type_error_jv(expected: &str) -> JitValue {
    eprintln!("JIT type error: expected {}", expected);
    JitValue::nil()
}

/// Overflow error helper for JIT arithmetic
fn overflow_error_jv(op: &str) -> JitValue {
    eprintln!("JIT overflow error: integer {} overflow", op);
    JitValue::nil()
}

mod arith;
mod compare;
mod ops;
pub use arith::*;
pub use compare::*;
pub use ops::*;

// =============================================================================
// Arithmetic Operations
// =============================================================================

#[cfg(test)]
mod tests;
