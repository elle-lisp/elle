pub mod bytecode;
pub mod bytecode_debug;

pub use bytecode::{Bytecode, Instruction};
pub use bytecode_debug::{disassemble, disassemble_lines, format_bytecode_with_constants};
