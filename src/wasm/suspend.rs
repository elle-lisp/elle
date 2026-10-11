// audited: 2026-10-06
//! CPS suspension and resume for WASM closures that may signal: where a frame
//! parks, the park itself, and spill and restore.
//!
//! docs/impl/wasm.md
//! docs/impl/region/unwind.md
//!
//! The blocks arrive already split after each park site
//! (`LirView::split_after` over `ParkSite::at`), so a resume enters at a block
//! boundary.

use crate::lir::code::{NodeRef, Op};
use crate::lir::{InstrRef, LirView, Reg, Terminator};
use wasm_encoder::*;

use super::emit::*;
use super::layout::{Case, Layout};

/// A place a signal can leave a function, so a function that may signal parks
/// there: every call, and every instruction that may raise
/// (`Op::may_raise`). An error unwinds nothing, so it parks the raising
/// frame as a suspend does (docs/impl/region/unwind.md).
#[derive(Clone, Copy)]
pub(super) struct ParkSite {
    /// The register a restart delivers into, or `None` for an instruction
    /// that produces no value (`Op::raise_delivers`).
    pub dst: Option<Reg>,
}

impl ParkSite {
    /// Whether an instruction of opcode `op` is a park site. A function that
    /// may signal splits its blocks after each one.
    pub(super) fn at(op: Op) -> bool {
        matches!(op, Op::Call | Op::SuspendingCall | Op::CallArrayMut) || op.may_raise()
    }

    /// The park site `node` is, if it is one.
    pub(super) fn of(node: &NodeRef<'_>) -> Option<ParkSite> {
        let op = node.op();
        ParkSite::at(op).then(|| ParkSite {
            dst: if op.raise_delivers() {
                node.def()
            } else {
                None
            },
        })
    }
}

/// What the instruction being emitted does with a signal its host operation
/// hands back.
#[derive(Clone, Copy)]
pub(super) enum SignalExit {
    /// Hand the signal to the caller through `SIGNAL_SLOT` and return with
    /// `status = 0`: the frame does not park.
    Return,
    /// Park the frame at `resume_state`, spilling the live set `spill_key`
    /// names.
    Park {
        resume_state: u32,
        spill_key: (usize, usize),
    },
}

impl WasmEmitter {
    /// Spill this frame's live state, save it as a suspension frame through
    /// `rt_yield`, and return parked at `resume_state`. `(tag, pay)` are the
    /// locals holding the value the park carries, and `signal` pushes the
    /// signal word it parks on.
    pub(super) fn emit_park(
        &self,
        f: &mut Function,
        (tag, pay): (u32, u32),
        resume_state: u32,
        spill_key: (usize, usize),
        signal: &Instruction,
    ) {
        let total_saved = self.num_regs + self.num_stack_locals;
        self.emit_spill(f, spill_key);
        f.instruction(&Instruction::LocalGet(tag));
        f.instruction(&Instruction::LocalGet(pay));
        f.instruction(&Instruction::I32Const(resume_state as i32));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(total_saved as i32));
        f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
        f.instruction(signal);
        f.instruction(&Instruction::Call(FN_RT_YIELD));
        f.instruction(&Instruction::LocalGet(tag));
        f.instruction(&Instruction::LocalGet(pay));
        f.instruction(&Instruction::I64Const(resume_state as i64));
        f.instruction(&Instruction::Return);
    }

    /// Emit `instr`, a park site, so that a signal leaving the frame there parks
    /// it at `resume_state`. `spill_key` names the site's live set
    /// (`liveness::compute_spill_liveness`).
    pub(super) fn emit_park_site(
        &mut self,
        f: &mut Function,
        instr: &InstrRef<'_>,
        resume_state: u32,
        spill_key: (usize, usize),
    ) {
        match instr {
            InstrRef::Call {
                dst, func, args, ..
            }
            | InstrRef::SuspendingCall {
                dst, func, args, ..
            } => {
                for (i, arg) in args.iter().enumerate() {
                    self.write_val_to_mem(f, *arg, i);
                }
                f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(args.len() as i32));
                f.instruction(&Instruction::I32Const(0));
                f.instruction(&Instruction::Call(FN_RT_CALL));
                self.park_on_call_outcome(f, *dst, resume_state, spill_key);
            }
            InstrRef::CallArrayMut {
                dst, func, args, ..
            } => {
                self.write_val_to_mem(f, *func, 0);
                self.write_val_to_mem(f, *args, 1);
                f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(-1));
                f.instruction(&Instruction::I32Const(0));
                f.instruction(&Instruction::Call(FN_RT_CALL));
                self.park_on_call_outcome(f, *dst, resume_state, spill_key);
            }
            _ => {
                self.signal_exit = SignalExit::Park {
                    resume_state,
                    spill_key,
                };
                self.emit_instr(f, instr);
                self.signal_exit = SignalExit::Return;
            }
        }
    }

    /// Store the four words `rt_call` returns into `dst`, and park the frame
    /// when the callee parked.
    fn park_on_call_outcome(
        &self,
        f: &mut Function,
        dst: Reg,
        resume_state: u32,
        spill_key: (usize, usize),
    ) {
        f.instruction(&Instruction::LocalSet(self.suspended_local()));
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));

        // Did the callee park? Ask the word the host set, never the signal:
        // which signals park is `signals::dispatch::parks`'s rule, and a
        // suspension need not carry any particular bit.
        f.instruction(&Instruction::LocalGet(self.suspended_local()));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::If(BlockType::Empty));
        self.emit_park(
            f,
            (self.tag_local(dst), self.pay_local(dst)),
            resume_state,
            spill_key,
            &Instruction::LocalGet(self.signal_local),
        );
        f.instruction(&Instruction::End);

        // Not a park, but still a signal: a halt. Hand it to the caller
        // through SIGNAL_SLOT with `status = 0`.
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(SIGNAL_SLOT));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Spill the live set `spill_key` names, or every slot when no live set was
    /// computed (`--wasm-no-sparse-spill`).
    pub(super) fn emit_spill(&self, f: &mut Function, spill_key: (usize, usize)) {
        if let Some(live_set) = self.spill_live_map.get(&spill_key) {
            self.emit_spill_live(f, live_set);
        } else {
            self.emit_spill_all(f);
        }
    }

    /// Spill only the slots live at this suspend point. `live_slots` is in the
    /// combined slot space `compute_spill_liveness` produces: register slots are
    /// `[0, num_regs)`, stack-local slots are `num_regs + local_slot`. A dead
    /// register or local is not spilled — the restore side reloads the whole
    /// saved frame, so any slot omitted here is simply reloaded stale into a
    /// dead local and never read.
    fn emit_spill_live(&self, f: &mut Function, live_slots: &std::collections::HashSet<u32>) {
        for i in 0..self.num_regs {
            if !live_slots.contains(&i) {
                continue;
            }
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
            if !live_slots.contains(&(self.num_regs + i)) {
                continue;
            }
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

    /// Emit the resume prologue of a function that may signal: on a resume,
    /// restore the saved slots and select the resume state's case; on a fresh
    /// call, select the entry block.
    pub(super) fn emit_resume_prologue(&self, f: &mut Function, layout: &Layout, entry: Case) {
        f.instruction(&Instruction::LocalGet(self.ctx_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            f.instruction(&Instruction::Call(FN_RT_GET_RESUME_VALUE));
            f.instruction(&Instruction::LocalSet(self.resume_pay_local));
            f.instruction(&Instruction::LocalSet(self.resume_tag_local));

            // Every resume state restores the same `total_saved` slots from the
            // front suspension frame, so the restore runs once, before the
            // dispatch, and the code stays linear in the slots rather than in
            // states times slots. A slot outside a state's live set lands in a
            // dead local that nothing reads.
            let total_saved = self.num_regs + self.num_stack_locals;
            self.emit_restore_all(f, total_saved);

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
                f.instruction(&Instruction::I64Const(layout.dispatch(info.target) as i64));
                f.instruction(&Instruction::LocalSet(self.signal_local));
                f.instruction(&Instruction::Br(idx as u32));
            }
            f.instruction(&Instruction::End);
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::I64Const(layout.dispatch(entry) as i64));
            f.instruction(&Instruction::LocalSet(self.signal_local));
        }
        f.instruction(&Instruction::End);
    }

    /// Pre-scan to build the resume_states table. Must be called before emit_cfg.
    pub(super) fn pre_scan_resume_states(&mut self, func: &LirView<'_>) {
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();
        self.next_resume_state = 1;
        for block in func.blocks() {
            let block_idx = self.label_to_idx[&block.label()];

            if let Terminator::Emit { resume_label, .. } = block.terminator() {
                let state_id = self.next_resume_state;
                self.next_resume_state += 1;
                self.resume_states.push(ResumeStateInfo {
                    target: Case::Block(self.label_to_idx[&resume_label]),
                });
                self.yield_state_map.insert(block_idx, state_id);
            }

            for (instr_idx, node) in block.nodes().enumerate() {
                if let Some(ParkSite { dst }) = ParkSite::of(&node) {
                    let state_id = self.next_resume_state;
                    self.next_resume_state += 1;
                    self.resume_states.push(ResumeStateInfo {
                        target: Case::Resume(self.call_continuations.len()),
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
