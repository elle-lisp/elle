//! # Elle - A High-Performance Lisp Interpreter
//!
//! Elle is a bytecode-compiled Lisp interpreter written in Rust with a register-based VM.
//!
//! ## Quick Start
//!
//! ```
//! use elle::{read_str, compile, register_primitives, SymbolTable, VM};
//! use elle::compiler::converters::value_to_expr;
//!
//! let mut vm = VM::new();
//! let mut symbols = SymbolTable::new();
//! register_primitives(&mut vm, &mut symbols);
//!
//! let code = "(+ 1 2 3)";
//! let value = read_str(code, &mut symbols).unwrap();
//! let expr = value_to_expr(&value, &mut symbols).unwrap();
//! let bytecode = compile(&expr);
//! let result = vm.execute(&bytecode).unwrap();
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

pub mod compiler;
pub mod error;
pub mod ffi;
pub mod ffi_primitives;
pub mod primitives;
pub mod reader;
pub mod symbol;
pub mod value;
pub mod vm;

pub use compiler::{compile, Bytecode};
pub use error::{RuntimeError, SourceLoc};
pub use primitives::{init_stdlib, register_primitives};
pub use reader::{read_str, Lexer, Reader};
pub use symbol::SymbolTable;
pub use value::{list, Value};
pub use vm::VM;
