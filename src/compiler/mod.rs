pub mod bytecode;
pub mod bytecode_debug;

pub use bytecode::{Bytecode, Instruction};
pub use bytecode_debug::{disassemble, disassemble_lines, format_bytecode_with_constants};
// Types re-exported from crate::symbols for backward compatibility
pub use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
