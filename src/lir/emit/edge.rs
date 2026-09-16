// audited: 2026-09-16
// src/lir/AGENTS.md
//! How a block exits, and what each edge owes the operand stack.
//!
//! A block inherits its stack simulation from the first predecessor that
//! reaches it, so that predecessor fixes the block's operand depth and every
//! later edge must arrive at the same depth (src/lir/AGENTS.md § "Merge operand
//! depth"). `Terminator::Jump` is the only exit that trims to meet the rule,
//! and the trim it takes depends on which way the edge runs.

use super::*;

impl Emitter {
    /// The operand depth `label` is already fixed at, or `None` when this edge
    /// is the first to reach it and so gets to fix it.
    ///
    /// A block's depth is decided by whichever predecessor is emitted first —
    /// the simulation keeps that predecessor's stack and discards every later
    /// one (see `Terminator::Jump`'s `or_insert_with`). The record lives in
    /// `yield_stack_state` while the block is still ahead of the cursor, and
    /// moves to `block_entry_depth` when `emit_block` consumes it, so a back
    /// edge into an already-emitted loop header is answered too.
    fn edge_depth(&self, label: Label) -> Option<usize> {
        self.yield_stack_state
            .get(&label)
            .map(|(stack, _)| stack.len())
            .or_else(|| self.block_entry_depth.get(&label).copied())
    }

    pub(super) fn emit_terminator(&mut self, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                self.ensure_on_top(*reg);
                self.bytecode.emit(Instruction::Return);
            }

            Terminator::Jump(label) => {
                // Every edge into a block must leave the depth that block was
                // fixed at, and a jump is the only exit that can trim to it.
                // The trim differs by direction (src/lir/AGENTS.md § "Merge
                // operand depth").
                match self.block_entry_depth.get(label).copied() {
                    // A BACK edge: the target is already emitted, so its
                    // simulation names only cells below `floor` and reads
                    // nothing this block pushed. Every surplus cell goes,
                    // orphan and live alike. The orphan trim alone cannot meet
                    // the rule here — it stops at the first live cell, and a
                    // body that leaves an orphan UNDER one deepens the header's
                    // stack once per trip, for as long as the loop runs.
                    Some(floor) => self.pop_to(floor),
                    // A FORWARD edge: the target is still ahead of the cursor,
                    // and a later edge's own result may sit above an orphan, so
                    // only the dead trailing cells go. Trimming past the depth
                    // splits the paths — a sibling `Terminator::Branch` edge
                    // pops nothing, and the merge's successors, which inherited
                    // the branch's simulation, would pop the orphan a second
                    // time on the path that already dropped it.
                    None => {
                        let floor = self.edge_depth(*label).unwrap_or(0);
                        self.pop_trailing_orphans_to(floor);
                    }
                }

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

                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((pos, *label));
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                self.ensure_on_top(*cond);

                // JumpIfFalse pops the condition from the stack
                self.pop();

                // Save stack state for both branches, but only if they haven't
                // been processed yet. This handles the case where blocks are
                // sorted by label and a target block might be processed before
                // the branch that jumps to it.
                if !self.label_offsets.contains_key(then_label) {
                    self.yield_stack_state
                        .insert(*then_label, (self.stack.clone(), self.reg_to_stack.clone()));
                }
                if !self.label_offsets.contains_key(else_label) {
                    self.yield_stack_state
                        .insert(*else_label, (self.stack.clone(), self.reg_to_stack.clone()));
                }

                // JumpIfFalse to else_label
                self.bytecode.emit(Instruction::JumpIfFalse);
                let else_pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((else_pos, *else_label));

                // Fall through or jump to then_label
                self.bytecode.emit(Instruction::Jump);
                let then_pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((then_pos, *then_label));
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

                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((pos, *resume_label));
            }

            Terminator::Unreachable => {
                // Emit nil and return as fallback
                self.bytecode.emit(Instruction::Nil);
                self.bytecode.emit(Instruction::Return);
            }
        }
    }
}
