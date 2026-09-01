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
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::Jump | Instruction::JumpIfFalse | Instruction::JumpIfTrue
                if i + 3 < instructions.len() =>
            {
                let offset = i32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                let target = (i + 4) as i64 + offset as i64;
                line.push_str(&format!(" (offset={}, target={})", offset, target));
                i += 4;
            }
            Instruction::LoadLocal | Instruction::StoreLocal if i + 1 < instructions.len() => {
                let index = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (index={})", index));
                i += 2;
            }
            Instruction::LoadUpvalue | Instruction::LoadUpvalueRaw | Instruction::StoreUpvalue
                if i + 2 < instructions.len() =>
            {
                let depth = instructions[i];
                let index = ((instructions[i + 1] as u16) << 8) | (instructions[i + 2] as u16);
                line.push_str(&format!(" (depth={}, index={})", depth, index));
                i += 3;
            }
            Instruction::Call | Instruction::CallChecked if i + 5 < instructions.len() => {
                let arg_count = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                let region_id = u32::from_be_bytes([
                    instructions[i + 2],
                    instructions[i + 3],
                    instructions[i + 4],
                    instructions[i + 5],
                ]);
                line.push_str(&format!(" (args={}, region={})", arg_count, region_id));
                i += 6;
            }
            // TailCall carries, after the region: the adopt-callee flag (1 byte —
            // release the callee closure's region at activation end), the
            // closure-cycle merged-arena adopt slot (u32, `0` = None), and the
            // borrowed-argument stash slots (a count byte then one u16 each).
            // See `LirInstr::TailCall::{defer_callee_release,
            // deferred_release_slot, borrowed_arg_slots}`.
            Instruction::TailCall | Instruction::TailCallChecked if i + 11 < instructions.len() => {
                let arg_count = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                let region_id = u32::from_be_bytes([
                    instructions[i + 2],
                    instructions[i + 3],
                    instructions[i + 4],
                    instructions[i + 5],
                ]);
                let adopt = instructions[i + 6];
                let adopt_slot = u32::from_be_bytes([
                    instructions[i + 7],
                    instructions[i + 8],
                    instructions[i + 9],
                    instructions[i + 10],
                ]);
                let borrowed = instructions[i + 11] as usize;
                line.push_str(&format!(
                    " (args={}, region={}, defer_callee_release={}, deferred_release_slot={}, \
                     borrowed_args={})",
                    arg_count, region_id, adopt, adopt_slot, borrowed
                ));
                i += 12 + borrowed * 2;
            }
            Instruction::DupN if i < instructions.len() => {
                let offset = instructions[i];
                line.push_str(&format!(" (offset={})", offset));
                i += 1;
            }
            Instruction::MakeClosure if i + 7 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                let const_idx = ((instructions[i + 4] as u16) << 8) | (instructions[i + 5] as u16);
                let num_captures =
                    ((instructions[i + 6] as u16) << 8) | (instructions[i + 7] as u16);
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
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (index={})", idx));
                i += 2;
            }
            Instruction::StructGetOrNil | Instruction::StructGetDestructure
                if i + 1 < instructions.len() =>
            {
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::StructRest if i + 1 < instructions.len() => {
                let count = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                i += 2;
                let mut keys = Vec::new();
                for _ in 0..count {
                    if i + 1 < instructions.len() {
                        let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
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
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                let args_region = u32::from_be_bytes([
                    instructions[i + 4],
                    instructions[i + 5],
                    instructions[i + 6],
                    instructions[i + 7],
                ]);
                line.push_str(&format!(
                    " (region={}, args_region={})",
                    region_id, args_region
                ));
                i += 8;
            }
            Instruction::IntToFloat | Instruction::FloatToInt => {
                // No operands — pop one, push one
            }
            Instruction::IncrefRegion if i + 3 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                line.push_str(&format!(" (region={})", region_id));
                i += 4;
            }
            Instruction::DecrefRegion if i + 3 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                line.push_str(&format!(" (region={})", region_id));
                i += 4;
            }
            // The coalescing oracle carries one u32 slot operand, like
            // Incref/DecrefRegion — skip it so the stream stays aligned.
            Instruction::AssertRegionMatches if i + 3 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                line.push_str(&format!(" (region={})", region_id));
                i += 4;
            }
            // Both signal-bits operands are eight bytes, big-endian —
            // `Bytecode::emit_signal_bits` writes them (docs/impl/bytecode.md
            // § "Signal-bits operands").
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
            Instruction::Pair | Instruction::MakeCapture if i + 3 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                if region_id != 0 {
                    line.push_str(&format!(" (region={})", region_id));
                }
                i += 4;
            }
            Instruction::IntrFreeze | Instruction::IntrThaw if i + 3 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                line.push_str(&format!(" (region={})", region_id));
                i += 4;
            }
            Instruction::MakeArrayMut if i + 4 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                let size = instructions[i + 4];
                line.push_str(&format!(" (region={}, size={})", region_id, size));
                i += 5;
            }
            Instruction::MaterializeConst if i + 7 < instructions.len() => {
                let region_id = u32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                // u32 template byte-length prefix lets us skip the inline
                // recursive template without decoding it.
                let len = u32::from_be_bytes([
                    instructions[i + 4],
                    instructions[i + 5],
                    instructions[i + 6],
                    instructions[i + 7],
                ]) as usize;
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
