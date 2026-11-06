// audited: 2026-11-05
// src/lir/AGENTS.md
//! How a block exits, and what each edge owes the operand stack.
//!
//! A block inherits its stack simulation from a predecessor, so the first
//! predecessor fixes the block's operand depth and every later edge must arrive
//! at the same depth (src/lir/AGENTS.md). Every edge trims to meet the rule —
//! a `Jump`, and each edge of a `Branch` on that edge's own path — and the trim
//! depends on which way the edge runs. A jump to a block already emitted runs
//! backward, and the interpreter charges fuel there, so it records a charge
//! site.

use super::*;

impl Emitter {
    /// The operand depth `label` is already fixed at, or `None` when this edge
    /// is the first to reach it and so gets to fix it.
    ///
    /// The record lives in `yield_stack_state` while the block is still ahead
    /// of the cursor, and moves to `block_entry_depth` when `emit_block`
    /// consumes it, so a back edge into an already-emitted loop header is
    /// answered too.
    fn edge_depth(&self, label: Label) -> Option<usize> {
        self.yield_stack_state
            .get(&label)
            .map(|(stack, _)| stack.len())
            .or_else(|| self.block_entry_depth.get(&label).copied())
    }

    /// Pop what the edge into `label` must not carry, so it arrives at the
    /// depth `label` was fixed at.
    fn trim_for_edge(&mut self, label: Label) {
        match self.block_entry_depth.get(&label).copied() {
            // A BACK edge: the target is already emitted, so its simulation
            // names only cells below `floor` and reads nothing this block
            // pushed. Every surplus cell goes, dead and live alike. The
            // dead-cell trim alone cannot meet the rule here — it stops at the
            // first cell that is not dead, and a body that leaves a dead cell
            // UNDER one deepens the header's stack once per trip, for as long
            // as the loop runs.
            Some(floor) => self.pop_to(floor),
            // A FORWARD edge: the target is still ahead of the cursor, and a
            // later edge's own result may sit above a dead cell, so only the
            // dead trailing cells go. Trimming past the depth splits the paths:
            // the merge's successors inherited the simulation of the edge that
            // fixed it, and would pop the dead cell a second time on the path
            // that already dropped it.
            None => {
                let floor = self.edge_depth(label).unwrap_or(0);
                self.pop_trailing_dead_to(floor, label);
            }
        }
    }

    /// Record the simulation as the one `label` starts from, unless `label`
    /// is already emitted: a loop header keeps the state it was emitted with.
    fn record_branch_edge(&mut self, label: Label) {
        if !self.label_offsets.contains_key(&label) {
            self.yield_stack_state
                .insert(label, (self.stack.clone(), self.reg_to_stack.clone()));
        }
    }

    pub(super) fn emit_terminator(&mut self, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                self.ensure_on_top(*reg);
                self.bytecode.emit(Instruction::Return);
            }

            Terminator::Jump(label) => {
                self.trim_for_edge(*label);

                // Save stack state for the target block if this is the first
                // predecessor to jump there. Multiple blocks may jump to the
                // same target (e.g., break + fallthrough, if/and/or merges).
                // We keep the FIRST saved state and ignore later ones — the
                // first predecessor is the reachable path (later predecessors
                // may be dead code after break with a wrong stack layout).
                if !self.label_offsets.contains_key(label) {
                    self.yield_stack_state
                        .entry(*label)
                        .or_insert_with(|| (self.stack.clone(), self.reg_to_stack.clone()));
                }

                self.emit_jump(*label);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                self.ensure_on_top(*cond);

                // JumpIfFalse pops the condition from the stack
                self.pop();
                let (stack, reg_to_stack) = (self.stack.clone(), self.reg_to_stack.clone());

                self.bytecode.emit(Instruction::JumpIfFalse);
                let else_pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder

                // The fall-through edge trims after JumpIfFalse, so only this
                // path pops. Its jump runs backward when then_label is a loop
                // header emitted before this block.
                self.trim_for_edge(*then_label);
                self.record_branch_edge(*then_label);
                self.emit_jump(*then_label);

                // The taken edge starts from the stack JumpIfFalse left. When it
                // has a cell to pop, JumpIfFalse lands on a pad that pops it,
                // then jumps to else_label.
                self.stack = stack;
                self.reg_to_stack = reg_to_stack;
                let pad = self.bytecode.current_pos();
                self.trim_for_edge(*else_label);
                self.record_branch_edge(*else_label);
                if self.bytecode.current_pos() == pad {
                    self.pending_jumps.push((else_pos, *else_label));
                } else {
                    // The same relative offset the label patch in `emit` writes.
                    self.bytecode
                        .patch_jump(else_pos, pad as i32 - else_pos as i32 - 4);
                    self.emit_jump(*else_label);
                }
            }

            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => {
                self.ensure_on_top(*value);
                // The whole mask is baked in: `(signal :keyword)` resolves to a
                // bit at analysis time, so nothing at runtime re-reads the
                // registry for a literal `emit`, and the operand is the only
                // place a user signal's bit (32-63) can live.
                self.bytecode.emit(Instruction::Emit);
                self.bytecode.emit_signal_bits(*signal);
                self.pop();

                let resume_ip = self.bytecode.current_pos();

                self.yield_points.push(YieldPointInfo {
                    resume_ip,
                    stack_regs: self.stack.clone(),
                    num_locals: self.current_func_num_locals,
                });

                self.yield_stack_state.insert(
                    *resume_label,
                    (self.stack.clone(), self.reg_to_stack.clone()),
                );

                self.emit_jump(*resume_label);
            }

            Terminator::Unreachable => {
                // Emit nil and return as fallback
                self.bytecode.emit(Instruction::Nil);
                self.bytecode.emit(Instruction::Return);
            }
        }
    }
}
