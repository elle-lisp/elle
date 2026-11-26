// audited: 2026-11-26
//! The bytecode instruction set, and the buffer the emitter writes it into.
//!
//! src/compiler/README.md

pub mod bytecode;

pub use bytecode::{
    disassemble, disassemble_lines, format_bytecode_with_constants, format_bytecode_with_protos,
    Bytecode, Instruction,
};
