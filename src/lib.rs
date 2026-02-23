#![allow(clippy::result_large_err)]

//! # Elle - A High-Performance Lisp Interpreter
//!
//! Elle is a bytecode-compiled Lisp interpreter written in Rust with a register-based VM.
//!
//! ## Quick Start
//!
//! ```
//! use elle::{eval, register_primitives, SymbolTable, VM};
//!
//! let mut vm = VM::new();
//! let mut symbols = SymbolTable::new();
//! register_primitives(&mut vm, &mut symbols);
//!
//! let code = "(+ 1 2 3)";
//! let result = eval(code, &mut symbols, &mut vm).unwrap();
//! ```
//!
//! ## Architecture
//!
//! Elle compiles Lisp code through several stages:
//!
//! 1. **Reader** - Parse S-expressions from text
//! 2. **Compiler** - Convert AST to bytecode
//! 3. **VM** - Execute bytecode with a stack-based interpreter
//!
//! ## Performance
//!
//! - Bytecode compilation eliminates tree-walking overhead
//! - Register-based VM for efficient instruction dispatch
//! - Symbol interning for O(1) symbol comparison
//! - SmallVec optimization to avoid heap allocation

pub mod arithmetic;
pub mod binding;
pub mod compiler;
pub mod effects;
pub mod error;
pub mod ffi;
pub mod formatter;
pub mod hir;
pub mod jit;
pub mod lint;
pub mod lir;
pub mod pipeline;
pub mod primitives;
pub mod reader;
pub mod repl;
pub mod symbol;
pub mod symbols;
pub mod syntax;
pub mod value;
pub mod vm;

// Re-export ffi primitives from the ffi module
pub use ffi::primitives as ffi_primitives;

pub use compiler::Bytecode;
pub use error::{RuntimeError, SourceLoc};
pub use lint::diagnostics::{Diagnostic, Severity};
pub use pipeline::{
    analyze, analyze_all, compile, compile_all, eval, AnalyzeResult, CompileResult,
};
pub use primitives::{init_stdlib, register_primitives};
pub use reader::{read_str, Lexer, Reader};
pub use symbol::SymbolTable;
pub use symbols::{get_primitive_documentation, SymbolDef, SymbolIndex, SymbolKind};
pub use value::{list, Value};
pub use vm::VM;
