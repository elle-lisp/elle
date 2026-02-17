//! Bytecode debugging utilities for understanding instruction sequences

use crate::compiler::bytecode::Instruction;

/// Disassemble bytecode with proper instruction names and operands
pub fn disassemble(instructions: &[u8]) -> String {
    let mut output = String::new();
    let mut i = 0;

    while i < instructions.len() {
        let byte = instructions[i];
        let instr: Instruction = unsafe { std::mem::transmute(byte) };

        output.push_str(&format!("  [{}] = {:?}", i, instr));
        i += 1;

        // Parse and display operands based on instruction type
        match instr {
            Instruction::LoadConst | Instruction::LoadGlobal | Instruction::StoreGlobal => {
                if i + 1 < instructions.len() {
                    let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    output.push_str(&format!(" (const_idx={})", idx));
                    i += 2;
                }
            }
            Instruction::Jump | Instruction::JumpIfFalse | Instruction::JumpIfTrue => {
                if i + 1 < instructions.len() {
                    let high = instructions[i] as i8 as i16;
                    let low = instructions[i + 1] as i16;
                    let offset = (high << 8) | (low & 0xFF);
                    let target = (i + 2) as i32 + offset as i32;
                    output.push_str(&format!(" (offset={}, target={})", offset, target));
                    i += 2;
                }
            }
            Instruction::LoadLocal | Instruction::StoreLocal => {
                if i + 1 < instructions.len() {
                    let depth = instructions[i];
                    let index = instructions[i + 1];
                    output.push_str(&format!(" (depth={}, index={})", depth, index));
                    i += 2;
                }
            }
            Instruction::LoadUpvalue | Instruction::LoadUpvalueRaw | Instruction::StoreUpvalue => {
                if i + 1 < instructions.len() {
                    let depth = instructions[i];
                    let index = instructions[i + 1];
                    output.push_str(&format!(" (depth={}, index={})", depth, index));
                    i += 2;
                }
            }
            Instruction::Call | Instruction::TailCall => {
                if i < instructions.len() {
                    let arg_count = instructions[i];
                    output.push_str(&format!(" (args={})", arg_count));
                    i += 1;
                }
            }
            Instruction::DupN => {
                if i < instructions.len() {
                    let offset = instructions[i];
                    output.push_str(&format!(" (offset={})", offset));
                    i += 1;
                }
            }
            Instruction::MakeClosure => {
                if i + 2 < instructions.len() {
                    let const_idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    let num_captures = instructions[i + 2];
                    output.push_str(&format!(
                        " (const_idx={}, num_captures={})",
                        const_idx, num_captures
                    ));
                    i += 3;
                }
            }
            _ => {}
        }

        output.push('\n');
    }

    output
}

/// Pretty print bytecode with constants
pub fn format_bytecode_with_constants(instructions: &[u8], constants: &[crate::Value]) -> String {
    let mut output = String::new();
    output.push_str("Bytecode:\n");
    output.push_str(&disassemble(instructions));
    output.push_str("\nConstants:\n");
    for (i, c) in constants.iter().enumerate() {
        output.push_str(&format!("  [{}] = {:?}\n", i, c));
    }
    output
}
