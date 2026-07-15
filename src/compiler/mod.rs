pub mod bytecode;

pub use bytecode::{
    disassemble, disassemble_lines, format_bytecode_with_constants, format_bytecode_with_protos,
    Bytecode, Instruction,
};
