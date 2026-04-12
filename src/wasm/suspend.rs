//! CPS suspension/resume machinery for yielding WASM closures.
//!
//! Handles spill/restore of registers across yield points, resume
//! prologue generation, block splitting at suspending call boundaries,
//! and yield-aware call emission.

use crate::lir::{BasicBlock, Label, LirFunction, LirInstr, Reg, SpannedTerminator, Terminator};
use wasm_encoder::*;

use super::emit::*;

impl WasmEmitter {
    /// Emit a function call in a suspending function.
    /// Like emit_call, but checks for SIG_YIELD before the general signal return.
    pub(super) fn emit_call_suspending(
        &self,
        f: &mut Function,
        dst: Reg,
        func: Reg,
        args: &[Reg],
        resume_state: u32,
    ) {
        for (i, arg) in args.iter().enumerate() {
            self.write_val_to_mem(f, *arg, i);
        }

        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));

        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));

        // Check SIG_YIELD (bit 1 = value 2)
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::I32And);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            let total_saved = self.num_regs + self.num_stack_locals;
            self.emit_spill_all(f);
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::I32Const(total_saved as i32));
            f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
            f.instruction(&Instruction::LocalGet(self.signal_local));
            f.instruction(&Instruction::Call(FN_RT_YIELD));
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);

        // Check other signals (error etc.)
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit CallArrayMut in a suspending function.
    pub(super) fn emit_call_array_suspending(
        &self,
        f: &mut Function,
        dst: Reg,
        func: Reg,
        args_array: Reg,
        resume_state: u32,
    ) {
        self.write_val_to_mem(f, func, 0);
        self.write_val_to_mem(f, args_array, 1);
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(-1));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));

        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));

        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::I32And);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            let total_saved = self.num_regs + self.num_stack_locals;
            self.emit_spill_all(f);
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::I32Const(total_saved as i32));
            f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
            f.instruction(&Instruction::LocalGet(self.signal_local));
            f.instruction(&Instruction::Call(FN_RT_YIELD));
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);

        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Spill all registers + local slots to linear memory at ARGS_BASE.
    pub(super) fn emit_spill_all(&self, f: &mut Function) {
        for i in 0..self.num_regs {
            let offset = (i * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.tag_phys(i)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.pay_phys(i)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }
        for i in 0..self.num_stack_locals {
            let offset = ((self.num_regs + i) * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.local_slot_tag(i as u16)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.local_slot_pay(i as u16)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }
    }

    /// Restore all registers + local slots from saved data via rt_load_saved_reg.
    fn emit_restore_all(&self, f: &mut Function, num_saved: u32) {
        let num_regs = self.num_regs.min(num_saved);
        let num_locals = (num_saved - num_regs).min(self.num_stack_locals);

        for i in 0..num_regs {
            f.instruction(&Instruction::I32Const(i as i32));
            f.instruction(&Instruction::Call(FN_RT_LOAD_SAVED_REG));
            f.instruction(&Instruction::LocalSet(self.pay_phys(i)));
            f.instruction(&Instruction::LocalSet(self.tag_phys(i)));
        }
        for i in 0..num_locals {
            f.instruction(&Instruction::I32Const((self.num_regs + i) as i32));
            f.instruction(&Instruction::Call(FN_RT_LOAD_SAVED_REG));
            f.instruction(&Instruction::LocalSet(self.local_slot_pay(i as u16)));
            f.instruction(&Instruction::LocalSet(self.local_slot_tag(i as u16)));
        }
    }

    /// Emit the resume prologue for suspending closures.
    pub(super) fn emit_resume_prologue(&self, f: &mut Function, state_local: u32) {
        let entry_idx = self.label_to_idx.values().min().copied().unwrap_or(0) as i32;

        f.instruction(&Instruction::LocalGet(self.ctx_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            f.instruction(&Instruction::Call(FN_RT_GET_RESUME_VALUE));
            f.instruction(&Instruction::LocalSet(self.resume_pay_local));
            f.instruction(&Instruction::LocalSet(self.resume_tag_local));

            let num_states = self.resume_states.len();
            f.instruction(&Instruction::Block(BlockType::Empty));
            for _ in 0..num_states {
                f.instruction(&Instruction::Block(BlockType::Empty));
            }

            f.instruction(&Instruction::LocalGet(self.ctx_local));
            f.instruction(&Instruction::I32Const(1));
            f.instruction(&Instruction::I32Sub);
            let targets: Vec<u32> = (0..num_states as u32)
                .map(|i| num_states as u32 - 1 - i)
                .collect();
            let default = if targets.is_empty() { 0 } else { targets[0] };
            f.instruction(&Instruction::BrTable(targets.into(), default));

            for idx in (0..num_states).rev() {
                f.instruction(&Instruction::End);
                let info = &self.resume_states[idx];
                self.emit_restore_all(f, info.num_saved);
                f.instruction(&Instruction::I32Const(info.target_block_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::Br(idx as u32));
            }
            f.instruction(&Instruction::End);
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::I32Const(entry_idx));
            f.instruction(&Instruction::LocalSet(state_local));
        }
        f.instruction(&Instruction::End);
    }

    /// Split blocks at SuspendingCall/CallArrayMut boundaries to avoid O(N²)
    /// code duplication in CPS virtual resume blocks.
    pub(super) fn split_blocks_at_suspending_calls(blocks: &[BasicBlock]) -> Vec<BasicBlock> {
        let max_label = blocks.iter().map(|b| b.label.0).max().unwrap_or(0);
        let mut next_label = max_label + 1;
        let mut result = Vec::new();

        for block in blocks {
            let call_positions: Vec<usize> = block
                .instructions
                .iter()
                .enumerate()
                .filter(|(i, si)| {
                    matches!(
                        si.instr,
                        LirInstr::SuspendingCall { .. } | LirInstr::CallArrayMut { .. }
                    ) && *i < block.instructions.len() - 1
                })
                .map(|(i, _)| i)
                .collect();

            if call_positions.is_empty() {
                result.push(block.clone());
                continue;
            }

            let mut start = 0;
            let mut current_label = block.label;

            for &call_pos in &call_positions {
                let cont_label = Label(next_label);
                next_label += 1;
                let mut split_block = BasicBlock::new(current_label);
                split_block.instructions = block.instructions[start..=call_pos].to_vec();
                split_block.terminator = SpannedTerminator::new(
                    Terminator::Jump(cont_label),
                    block.terminator.span.clone(),
                );
                result.push(split_block);
                start = call_pos + 1;
                current_label = cont_label;
            }

            let mut final_block = BasicBlock::new(current_label);
            final_block.instructions = block.instructions[start..].to_vec();
            final_block.terminator = block.terminator.clone();
            result.push(final_block);
        }

        result
    }

    /// Pre-scan to build the resume_states table. Must be called before emit_cfg.
    pub(super) fn pre_scan_resume_states(&mut self, func: &LirFunction) {
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();
        self.next_resume_state = 1;
        let total_saved = self.num_regs + self.num_stack_locals;
        let num_real_blocks = func.blocks.len();

        for block in &func.blocks {
            let block_idx = self.label_to_idx[&block.label];

            if let Terminator::Emit { resume_label, .. } = &block.terminator.terminator {
                let state_id = self.next_resume_state;
                self.next_resume_state += 1;
                let target_block_idx = self.label_to_idx[resume_label] as i32;
                self.resume_states.push(ResumeStateInfo {
                    state_id,
                    target_block_idx,
                    num_saved: total_saved,
                });
                self.yield_state_map.insert(block_idx, state_id);
            }

            for (instr_idx, spanned) in block.instructions.iter().enumerate() {
                let dst = match &spanned.instr {
                    LirInstr::SuspendingCall { dst, .. } => Some(*dst),
                    LirInstr::CallArrayMut { dst, .. } => Some(*dst),
                    _ => None,
                };
                if let Some(dst) = dst {
                    let state_id = self.next_resume_state;
                    self.next_resume_state += 1;
                    let virtual_idx = (num_real_blocks + self.call_continuations.len()) as i32;
                    self.resume_states.push(ResumeStateInfo {
                        state_id,
                        target_block_idx: virtual_idx,
                        num_saved: total_saved,
                    });
                    self.call_state_map.insert((block_idx, instr_idx), state_id);
                    self.call_continuations.push(CallSiteContinuation {
                        dst,
                        source_block_idx: block_idx,
                        instr_offset: instr_idx + 1,
                    });
                }
            }
        }
    }
}
