pub mod bytecode;

pub use bytecode::{
    disassemble, disassemble_lines, format_bytecode_with_constants, Bytecode, Instruction,
};
