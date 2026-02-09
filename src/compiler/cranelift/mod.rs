// Cranelift JIT compiler for Elle Lisp
//
// This module integrates Cranelift as a JIT backend to compile Elle Lisp
// functions directly to native x86_64 code, replacing the stack-based
// bytecode interpreter for hot functions.
//
// Architecture:
// 1. AST → Cranelift IR (CLIF)
// 2. Cranelift IR → x86_64 machine code
// 3. Runtime: Profile → JIT compile → Execute native
//
// Value Representation:
// - Values are `Value` enum (Rust side)
// - Passed by reference/pointer across native boundaries
// - Primitives (Int, Float, Bool, Nil) optimized as inline values

pub mod adaptive_compiler;
pub mod advanced_optimizer;
pub mod binop;
pub mod branching;
pub mod closure_compiler;
pub mod codegen;
pub mod compiler;
pub mod compiler_v2;
pub mod compiler_v3;
pub mod compiler_v3_stack;
pub mod compiler_v4;
pub mod context;
pub mod e2e_test;
pub mod escape_analyzer;
pub mod expr_compiler;
pub mod feedback_compiler;
pub mod funcall;
pub mod function_compiler;
pub mod optimizer;
pub mod phase10_milestone;
pub mod phase11_milestone;
pub mod phase12_milestone;
pub mod phase13_milestone;
pub mod phase14_milestone;
pub mod phase15_milestone;
pub mod primitives;
pub mod profiler;
pub mod scoping;
pub mod stack_allocator;
pub mod tests;
pub mod type_specializer;

pub use binop::BinOpCompiler;
pub use branching::BranchManager;
pub use codegen::IrEmitter;
pub use compiler::ExprCompiler;
pub use compiler_v3::ExprCompilerV3;
pub use context::JITContext;
pub use funcall::FunctionCallCompiler;
pub use primitives::{CompiledValue, PrimitiveEncoder};
