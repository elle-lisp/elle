// audited: 2026-11-13
//! Bytecode disassembly: decode each instruction's operands into one line.
//!
//! docs/impl/bytecode.md

use super::*;

/// Disassemble bytecode and return one string per instruction
pub fn disassemble_lines(instructions: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut i = 0;

    while i < instructions.len() {
        let byte = instructions[i];
        let Some(instr) = Instruction::from_byte(byte) else {
            lines.push(format!("[{}] Unknown(0x{:02x})", i, byte));
            i += 1;
            continue;
        };
        let mut line = format!("[{}] {:?}", i, instr);
        i += 1;

        match instr {
            Instruction::LoadConst if i + 1 < instructions.len() => {
                let idx = u16_at(instructions, i);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::Jump | Instruction::JumpIfFalse | Instruction::JumpIfTrue
                if i + 3 < instructions.len() =>
            {
                let offset = u32_at(instructions, i) as i32;
                let target = (i + 4) as i64 + offset as i64;
                line.push_str(&format!(" (offset={}, target={})", offset, target));
                i += 4;
            }
            Instruction::LoadLocal | Instruction::StoreLocal if i + 1 < instructions.len() => {
                let index = u16_at(instructions, i);
                line.push_str(&format!(" (index={})", index));
                i += 2;
            }
            Instruction::LoadUpvalue | Instruction::LoadUpvalueRaw | Instruction::StoreUpvalue
                if i + 2 < instructions.len() =>
            {
                let depth = instructions[i];
                let index = u16_at(instructions, i + 1);
                line.push_str(&format!(" (depth={}, index={})", depth, index));
                i += 3;
            }
            // Call carries, after the region, its moves: a count byte, then a
            // position byte and a u16 slot each (`InstrRef::Call::moves`).
            Instruction::Call | Instruction::CallChecked if i + 6 < instructions.len() => {
                let arg_count = u16_at(instructions, i);
                let region_id = u32_at(instructions, i + 2);
                i += 6;
                let moves = offers(instructions, &mut i);
                line.push_str(&format!(
                    " (args={}, region={}, moves={})",
                    arg_count, region_id, moves
                ));
            }
            // TailCall carries, after the region: the adopt-callee flag (1 byte —
            // release the callee closure's region at activation end), the
            // closure-cycle merged-arena adopt slot (u32, `0` = None), the
            // borrowed-argument stash slots (a count byte then one u16 each), and
            // the late mints, laid out as a Call's moves. See
            // `InstrRef::TailCall::{defer_callee_release, deferred_release_slot,
            // borrowed_arg_slots, late_mints}`.
            Instruction::TailCall | Instruction::TailCallChecked if i + 11 < instructions.len() => {
                let arg_count = u16_at(instructions, i);
                let region_id = u32_at(instructions, i + 2);
                let adopt = instructions[i + 6];
                let adopt_slot = u32_at(instructions, i + 7);
                let borrowed = instructions[i + 11] as usize;
                i += 12 + borrowed * 2;
                let late = offers(instructions, &mut i);
                line.push_str(&format!(
                    " (args={}, region={}, defer_callee_release={}, deferred_release_slot={}, \
                     borrowed_args={}, late_mints={})",
                    arg_count, region_id, adopt, adopt_slot, borrowed, late
                ));
            }
            Instruction::DupN if i < instructions.len() => {
                let offset = instructions[i];
                line.push_str(&format!(" (offset={})", offset));
                i += 1;
            }
            Instruction::MakeClosure if i + 7 < instructions.len() => {
                let region_id = u32_at(instructions, i);
                let const_idx = u16_at(instructions, i + 4);
                let num_captures = u16_at(instructions, i + 6);
                line.push_str(&format!(
                    " (region={}, const_idx={}, num_captures={})",
                    region_id, const_idx, num_captures
                ));
                i += 8;
            }
            Instruction::ArrayMutRefDestructure
            | Instruction::ArrayMutSliceFrom
            | Instruction::ArrayMutRefOrNil
                if i + 1 < instructions.len() =>
            {
                let idx = u16_at(instructions, i);
                line.push_str(&format!(" (index={})", idx));
                i += 2;
            }
            Instruction::StructGetOrNil | Instruction::StructGetDestructure
                if i + 1 < instructions.len() =>
            {
                let idx = u16_at(instructions, i);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::StructRest if i + 1 < instructions.len() => {
                let count = u16_at(instructions, i);
                i += 2;
                let mut keys = Vec::new();
                for _ in 0..count {
                    if i + 1 < instructions.len() {
                        let idx = u16_at(instructions, i);
                        i += 2;
                        keys.push(format!("const[{}]", idx));
                    }
                }
                line.push_str(&format!(" (count={}, keys=[{}])", count, keys.join(", ")));
            }
            Instruction::Eval => {
                // No operands — pops 2 from stack, pushes 1
            }
            Instruction::ArrayMutExtend | Instruction::ArrayMutPush => {
                // No operands
            }
            Instruction::CallArrayMut | Instruction::TailCallArrayMut
                if i + 7 < instructions.len() =>
            {
                let region_id = u32_at(instructions, i);
                let args_region = u32_at(instructions, i + 4);
                line.push_str(&format!(
                    " (region={}, args_region={})",
                    region_id, args_region
                ));
                i += 8;
            }
            Instruction::IntToFloat | Instruction::FloatToInt => {
                // No operands — pop one, push one
            }
            // One u32 region slot. The coalescing oracle carries one too, so
            // skipping it keeps the stream aligned.
            Instruction::IncrefRegion
            | Instruction::DecrefRegion
            | Instruction::AssertRegionMatches
            | Instruction::IntrFreeze
            | Instruction::IntrThaw
                if i + 3 < instructions.len() =>
            {
                let region_id = u32_at(instructions, i);
                line.push_str(&format!(" (region={})", region_id));
                i += 4;
            }
            // Both signal-bits operands are eight bytes, big-endian —
            // `Bytecode::emit_signal_bits` writes them (docs/impl/bytecode.md).
            Instruction::Emit if i + 7 < instructions.len() => {
                let raw = u64::from_be_bytes(instructions[i..i + 8].try_into().expect("8 bytes"));
                line.push_str(&format!(" (signal_bits=0x{:016x})", raw));
                i += 8;
            }
            Instruction::CheckSignalBound if i + 7 < instructions.len() => {
                let raw = u64::from_be_bytes(instructions[i..i + 8].try_into().expect("8 bytes"));
                line.push_str(&format!(" (allowed_bits=0x{:016x})", raw));
                i += 8;
            }
            Instruction::PushParamFrame if i < instructions.len() => {
                let count = instructions[i];
                line.push_str(&format!(" (count={})", count));
                i += 1;
            }
            Instruction::FreeRegionGroup if i < instructions.len() => {
                let count = instructions[i];
                line.push_str(&format!(" (count={})", count));
                i += 1;
            }
            Instruction::PopParamFrame => {
                // No operands
            }
            Instruction::Pair if i + 3 < instructions.len() => {
                let region_id = u32_at(instructions, i);
                if region_id != 0 {
                    line.push_str(&format!(" (region={})", region_id));
                }
                i += 4;
            }
            Instruction::MakeCapture if i + 12 < instructions.len() => {
                let region_id = u32_at(instructions, i);
                let mutated = instructions[i + 4];
                let name =
                    u64::from_be_bytes(instructions[i + 5..i + 13].try_into().expect("8 bytes"));
                line.push_str(&format!(
                    " (region={}, mutated={}, name=0x{:016x})",
                    region_id, mutated, name
                ));
                i += 13;
            }
            Instruction::MakeArrayMut if i + 4 < instructions.len() => {
                let region_id = u32_at(instructions, i);
                let size = instructions[i + 4];
                line.push_str(&format!(" (region={}, size={})", region_id, size));
                i += 5;
            }
            Instruction::MaterializeConst if i + 7 < instructions.len() => {
                let region_id = u32_at(instructions, i);
                // u32 template byte-length prefix lets us skip the inline
                // recursive template without decoding it.
                let len = u32_at(instructions, i + 4) as usize;
                i += 8;
                let end = (i + len).min(instructions.len());
                line.push_str(&format!(" (region={}, template_bytes={})", region_id, len));
                i = end;
            }
            _ => {}
        }

        lines.push(line);
    }

    lines
}

/// The big-endian `u16` at `i`. Each caller has checked the bounds.
fn u16_at(instructions: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([instructions[i], instructions[i + 1]])
}

/// The big-endian `u32` at `i`. Each caller has checked the bounds.
fn u32_at(instructions: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([
        instructions[i],
        instructions[i + 1],
        instructions[i + 2],
        instructions[i + 3],
    ])
}

/// A call's offers at `i`, as `[position:slot ...]`, stepping `i` past them.
fn offers(instructions: &[u8], i: &mut usize) -> String {
    let n = instructions.get(*i).copied().unwrap_or(0) as usize;
    *i += 1;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        if let Some(e) = instructions.get(*i..*i + 3) {
            entries.push(format!("{}:{}", e[0], u16::from_be_bytes([e[1], e[2]])));
        }
        *i += 3;
    }
    format!("[{}]", entries.join(" "))
}
