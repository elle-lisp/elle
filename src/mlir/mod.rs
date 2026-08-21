//! MLIR backend for Elle.
//!
//! Lowers GPU-eligible `LirFunction`s to MLIR using the melior crate,
//! then compiles through the arith/func/cf dialects to LLVM IR and
//! JIT-executes via the MLIR ExecutionEngine.

mod cache;
mod execute;
mod lower;
mod spirv;

pub use cache::MlirCache;
pub use execute::mlir_call;
pub use lower::{check_slot_types, lower_to_mlir, ScalarType};
pub use spirv::lower_to_spirv;

#[cfg(test)]
mod tests;
