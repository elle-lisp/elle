//! CFG emission: loop+br_table dispatch, block instructions, terminators.

use crate::lir::{LirFunction, LirInstr, Terminator};
use crate::value::repr::*;
use wasm_encoder::*;

use super::emit::*;

impl WasmEmitter {
    /// Emit a control flow graph using loop + br_table dispatch.
    ///
    /// Each LIR basic block becomes a case in a br_table. A `$state` local
    /// tracks which block to execute next. Return terminators break out of
    /// the loop. Jump and Branch terminators set `$state` and continue.
    pub(super) fn emit_cfg(&mut self, f: &mut Function, func: &LirFunction) {
        let num_blocks = func.blocks.len();
        if num_blocks == 0 {
            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::I64Const(0));
            return;
        }

        if num_blocks == 1 && !self.may_suspend {
            let block = &func.blocks[0];
            for spanned in &block.instructions {
                self.emit_instr(f, &spanned.instr);
            }
            self.emit_terminator_return(f, &block.terminator.terminator);
            return;
        }

        let state_local = self.signal_local;

        // Resume prologue
        if self.may_suspend && !self.resume_states.is_empty() {
            self.emit_resume_prologue(f, state_local);
        } else {
            let entry_idx = self.label_to_idx[&func.entry] as i64;
            f.instruction(&Instruction::I64Const(entry_idx));
            f.instruction(&Instruction::LocalSet(state_local));
        }

        let num_virtual = self.call_continuations.len();
        let total_blocks = num_blocks + num_virtual;

        f.instruction(&Instruction::Loop(BlockType::Empty));

        for _ in 0..total_blocks {
            f.instruction(&Instruction::Block(BlockType::Empty));
        }

        // br_table dispatch (BrTable needs i32, state_local is i64)
        f.instruction(&Instruction::LocalGet(state_local));
        f.instruction(&Instruction::I32WrapI64);
        let targets: Vec<u32> = (0..total_blocks as u32)
            .map(|i| total_blocks as u32 - 1 - i)
            .collect();
        let default = if targets.is_empty() { 0 } else { targets[0] };
        f.instruction(&Instruction::BrTable(targets.into(), default));

        // Virtual resume blocks (highest indices, innermost)
        for virt_idx in (0..num_virtual).rev() {
            f.instruction(&Instruction::End);

            let cont = &self.call_continuations[virt_idx];
            let src_block_idx = cont.source_block_idx;
            let instr_offset = cont.instr_offset;
            let dst = cont.dst;

            // Load resume value into call dst register
            f.instruction(&Instruction::LocalGet(self.resume_tag_local));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.resume_pay_local));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));

            // Emit remaining instructions from the source block
            let block = &func.blocks[src_block_idx];
            for (rel_idx, spanned) in block.instructions[instr_offset..].iter().enumerate() {
                let abs_idx = instr_offset + rel_idx;
                if self.may_suspend {
                    match &spanned.instr {
                        LirInstr::SuspendingCall {
                            dst: call_dst,
                            func: fn_reg,
                            args,
                            ..
                        } => {
                            let resume_state = self
                                .call_state_map
                                .get(&(src_block_idx, abs_idx))
                                .copied()
                                .unwrap_or(0);
                            self.emit_call_suspending(
                                f,
                                *call_dst,
                                *fn_reg,
                                args,
                                resume_state,
                                (src_block_idx, abs_idx),
                            );
                            continue;
                        }
                        LirInstr::CallArrayMut {
                            dst: call_dst,
                            func: fn_reg,
                            args,
                        } => {
                            let resume_state = self
                                .call_state_map
                                .get(&(src_block_idx, abs_idx))
                                .copied()
                                .unwrap_or(0);
                            self.emit_call_array_suspending(
                                f,
                                *call_dst,
                                *fn_reg,
                                *args,
                                resume_state,
                                (src_block_idx, abs_idx),
                            );
                            continue;
                        }
                        _ => {}
                    }
                }
                self.emit_instr(f, &spanned.instr);
            }

            self.emit_block_terminator(
                f,
                &block.terminator.terminator,
                state_local,
                (num_blocks + virt_idx) as u32,
                Some(src_block_idx),
            );
        }

        // Real blocks in reverse order
        for block_idx in (0..num_blocks).rev() {
            f.instruction(&Instruction::End);
            let block = &func.blocks[block_idx];
            self.emit_block_instructions(f, block_idx, func);
            self.emit_block_terminator(
                f,
                &block.terminator.terminator,
                state_local,
                block_idx as u32,
                Some(block_idx),
            );
        }

        f.instruction(&Instruction::End); // loop
        f.instruction(&Instruction::Unreachable);
    }

    /// Emit a block's instructions with yield-through checks for suspending calls.
    pub(super) fn emit_block_instructions(
        &mut self,
        f: &mut Function,
        block_idx: usize,
        func: &LirFunction,
    ) {
        self.known_int.clear();
        let block = &func.blocks[block_idx];
        for (instr_idx, spanned) in block.instructions.iter().enumerate() {
            if self.may_suspend {
                match &spanned.instr {
                    LirInstr::SuspendingCall {
                        dst,
                        func: fn_reg,
                        args,
                        ..
                    } => {
                        let resume_state = self
                            .call_state_map
                            .get(&(block_idx, instr_idx))
                            .copied()
                            .unwrap_or(0);
                        self.emit_call_suspending(
                            f,
                            *dst,
                            *fn_reg,
                            args,
                            resume_state,
                            (block_idx, instr_idx),
                        );
                        continue;
                    }
                    LirInstr::CallArrayMut {
                        dst,
                        func: fn_reg,
                        args,
                    } => {
                        let resume_state = self
                            .call_state_map
                            .get(&(block_idx, instr_idx))
                            .copied()
                            .unwrap_or(0);
                        self.emit_call_array_suspending(
                            f,
                            *dst,
                            *fn_reg,
                            *args,
                            resume_state,
                            (block_idx, instr_idx),
                        );
                        continue;
                    }
                    _ => {}
                }
            }
            self.emit_instr(f, &spanned.instr);
        }
    }

    /// Emit a block terminator.
    pub(super) fn emit_block_terminator(
        &mut self,
        f: &mut Function,
        term: &Terminator,
        state_local: u32,
        loop_depth: u32,
        lir_block_idx: Option<usize>,
    ) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::Return);
            }
            Terminator::Jump(target) => {
                let target_idx = self.label_to_idx[target] as i64;
                f.instruction(&Instruction::I64Const(target_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::Br(loop_depth));
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let then_idx = self.label_to_idx[then_label] as i32;
                let else_idx = self.label_to_idx[else_label] as i32;
                self.emit_truthiness_check(f, *cond);
                f.instruction(&Instruction::If(BlockType::Empty));
                f.instruction(&Instruction::I64Const(then_idx as i64));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::Else);
                f.instruction(&Instruction::I64Const(else_idx as i64));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::End);
                f.instruction(&Instruction::Br(loop_depth));
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            Terminator::Emit { signal, value, .. } => {
                if self.may_suspend {
                    let resume_state = lir_block_idx
                        .and_then(|idx| self.yield_state_map.get(&idx).copied())
                        .unwrap_or_else(|| {
                            let s = self.next_resume_state;
                            self.next_resume_state += 1;
                            s
                        });
                    let total_saved = self.num_regs + self.num_stack_locals;
                    let spill_key = (lir_block_idx.unwrap_or(0), usize::MAX);
                    self.emit_spill(f, spill_key);
                    f.instruction(&Instruction::LocalGet(self.tag_local(*value)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*value)));
                    f.instruction(&Instruction::I32Const(resume_state as i32));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(total_saved as i32));
                    f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
                    f.instruction(&Instruction::I64Const(signal.raw() as i64));
                    f.instruction(&Instruction::Call(FN_RT_YIELD));
                    f.instruction(&Instruction::LocalGet(self.tag_local(*value)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*value)));
                    f.instruction(&Instruction::I64Const(resume_state as i64));
                    f.instruction(&Instruction::Return);
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
        }
    }

    /// Emit a return terminator for single-block functions.
    pub(super) fn emit_terminator_return(&self, f: &mut Function, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::I64Const(0));
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            _ => {
                f.instruction(&Instruction::Unreachable);
            }
        }
    }
}
