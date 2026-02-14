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

pub mod binop;
pub mod branching;
pub mod codegen;
pub mod compiler;
pub mod context;
pub mod funcall;
pub mod jit_compile;
pub mod primitive_registry;
pub mod primitives;
pub mod profiler;
pub mod runtime_helpers;
pub mod scoping;
pub mod stack_allocator;

pub use binop::BinOpCompiler;
pub use branching::BranchManager;
pub use codegen::IrEmitter;
pub use compiler::ExprCompiler;
pub use context::JITContext;
pub use funcall::FunctionCallCompiler;
pub use jit_compile::{compile_closure, is_jit_compilable, CompileResult};
pub use primitive_registry::PrimitiveRegistry;
pub use primitives::{CompiledValue, PrimitiveEncoder};
