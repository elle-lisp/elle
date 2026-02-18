pub mod analysis;
pub mod ast;
pub mod bytecode;
pub mod bytecode_debug;
pub mod capture_resolution;
pub mod compile;
pub mod converters;
pub mod cps;
pub mod cranelift;
pub mod jit_coordinator;
pub mod jit_executor;
pub mod jit_wrapper;
pub mod linter;
pub mod macros;
pub mod optimize;
pub mod patterns;
pub mod symbol_index;

pub use bytecode::{Bytecode, Instruction};
pub use bytecode_debug::{disassemble, format_bytecode_with_constants};
pub use converters::value_to_expr;
pub use cps::{Action, Continuation};
pub use jit_coordinator::JitCoordinator;
pub use jit_executor::JitExecutor;
pub use jit_wrapper::{compile_jit, is_jit_compilable, JitCompiledFunction};
pub use linter::Linter;
pub use symbol_index::extract_symbols;
// Types re-exported from crate::symbols for backward compatibility
pub use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
