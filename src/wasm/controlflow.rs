// audited: 2026-10-06
//! CFG emission: the dispatch loop, the instructions of each case, and the terminators.
//!
//! docs/impl/wasm.md

use crate::lir::{Label, LirView, Terminator};
use crate::value::repr::*;
use wasm_encoder::*;

use super::emit::*;
use super::layout::{Case, Goto, Layout};
use super::suspend::ParkSite;

impl WasmEmitter {
    /// Emit a control flow graph as a dispatch loop.
    ///
    /// Each LIR block, and each park site's resume entry, is a case the
    /// `br_table` selects by the dispatch local, in the order `Layout` gives.
    /// A `Return` leaves the function. A `Jump` or `Branch` reaches a later
    /// case by a plain `br`, and an earlier one by setting the dispatch local
    /// and going round the loop.
    pub(super) fn emit_cfg(&mut self, f: &mut Function, func: &LirView<'_>) {
        let num_blocks = func.block_count();
        if num_blocks == 0 {
            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::I64Const(0));
            return;
        }

        if num_blocks == 1 && !self.may_suspend {
            let block = func.block(0);
            for instr in block.instrs() {
                self.emit_instr(f, &instr);
            }
            self.emit_terminator_return(f, &block.terminator());
            return;
        }

        let layout = Layout::new(
            num_blocks,
            self.call_continuations.iter().map(|c| c.source_block_idx),
        );
        let entry = Case::Block(self.label_to_idx[&func.entry()]);

        if self.may_suspend && !self.resume_states.is_empty() {
            self.emit_resume_prologue(f, &layout, entry);
        } else {
            f.instruction(&Instruction::I64Const(layout.dispatch(entry) as i64));
            f.instruction(&Instruction::LocalSet(self.signal_local));
        }

        f.instruction(&Instruction::Loop(BlockType::Empty));
        for _ in layout.cases() {
            f.instruction(&Instruction::Block(BlockType::Empty));
        }

        // br_table dispatch (BrTable needs i32, the dispatch local is i64)
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32WrapI64);
        f.instruction(&Instruction::BrTable(layout.table().into(), 0));

        for (pos, &case) in layout.cases().iter().enumerate() {
            f.instruction(&Instruction::End);
            let block_idx = match case {
                Case::Block(block_idx) => {
                    self.emit_block_instructions(f, block_idx, 0, func);
                    block_idx
                }
                Case::Resume(c) => {
                    let cont = &self.call_continuations[c];
                    let (dst, block_idx, start) =
                        (cont.dst, cont.source_block_idx, cont.instr_offset);
                    // Deliver the resume value into the site's result, where it
                    // has one.
                    if let Some(dst) = dst {
                        f.instruction(&Instruction::LocalGet(self.resume_tag_local));
                        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                        f.instruction(&Instruction::LocalGet(self.resume_pay_local));
                        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
                    }
                    self.emit_block_instructions(f, block_idx, start, func);
                    block_idx
                }
            };
            let term = func.block(block_idx).terminator();
            self.emit_block_terminator(f, &term, &layout, pos, block_idx);
        }

        f.instruction(&Instruction::End); // loop
        f.instruction(&Instruction::Unreachable);
    }

    /// Emit a block's instructions from `start`. In a function that may
    /// signal, each park site parks the frame at its resume state
    /// (`suspend::ParkSite`). What the emitter knows to be an integer is
    /// forgotten first: a block is entered from more than one place, and a
    /// continuation's result arrives from the resume.
    pub(super) fn emit_block_instructions(
        &mut self,
        f: &mut Function,
        block_idx: usize,
        start: usize,
        func: &LirView<'_>,
    ) {
        self.known_int.clear();
        let block = func.block(block_idx);
        for (instr_idx, node) in block.nodes().enumerate().skip(start) {
            let instr = node.instr();
            if self.may_suspend && ParkSite::of(&node).is_some() {
                let resume_state = *self
                    .call_state_map
                    .get(&(block_idx, instr_idx))
                    .expect("every park site has a resume state (pre_scan_resume_states)");
                self.emit_park_site(f, &instr, resume_state, (block_idx, instr_idx));
                continue;
            }
            self.emit_instr(f, &instr);
        }
    }

    /// Emit the terminator of the block at `block_idx`, closing the case at
    /// position `pos` of `layout`.
    pub(super) fn emit_block_terminator(
        &mut self,
        f: &mut Function,
        term: &Terminator,
        layout: &Layout,
        pos: usize,
        block_idx: usize,
    ) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::Return);
            }
            Terminator::Jump(target) => self.emit_goto(f, layout, pos, *target, 0),
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                self.emit_truthiness_check(f, *cond);
                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_goto(f, layout, pos, *then_label, 1);
                f.instruction(&Instruction::End);
                self.emit_goto(f, layout, pos, *else_label, 0);
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            Terminator::Emit { signal, value, .. } => {
                if self.may_suspend {
                    let resume_state = self
                        .yield_state_map
                        .get(&block_idx)
                        .copied()
                        .unwrap_or_else(|| {
                            let s = self.next_resume_state;
                            self.next_resume_state += 1;
                            s
                        });
                    let spill_key = (block_idx, usize::MAX);
                    // The gate answers the value and signal the emit parks on: a
                    // denial's where the fiber withholds a bit the emit names. They
                    // land in the park pair and the dispatch word, which the park
                    // leaves behind, so `value` stays as the continuation reads it.
                    f.instruction(&Instruction::LocalGet(self.tag_local(*value)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*value)));
                    f.instruction(&Instruction::I64Const(signal.raw() as i64));
                    f.instruction(&Instruction::Call(FN_RT_EMIT));
                    f.instruction(&Instruction::LocalSet(self.signal_local));
                    f.instruction(&Instruction::LocalSet(self.park_pay_local));
                    f.instruction(&Instruction::LocalSet(self.park_tag_local));
                    self.emit_park(
                        f,
                        (self.park_tag_local, self.park_pay_local),
                        resume_state,
                        spill_key,
                        &Instruction::LocalGet(self.signal_local),
                    );
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
        }
    }

    /// Branch from the case at position `from`, `nesting` structured blocks
    /// deep, to the block `target` labels.
    fn emit_goto(
        &self,
        f: &mut Function,
        layout: &Layout,
        from: usize,
        target: Label,
        nesting: u32,
    ) {
        let to = Case::Block(self.label_to_idx[&target]);
        match layout.goto(from, to, nesting) {
            Goto::Forward(depth) => {
                f.instruction(&Instruction::Br(depth));
            }
            Goto::Loop { dispatch, depth } => {
                f.instruction(&Instruction::I64Const(dispatch as i64));
                f.instruction(&Instruction::LocalSet(self.signal_local));
                f.instruction(&Instruction::Br(depth));
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
