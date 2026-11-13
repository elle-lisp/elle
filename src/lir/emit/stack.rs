// audited: 2026-11-13
//! Stack simulation helpers for the LIR emitter.
//!
//! src/lir/AGENTS.md
//!
//! Tracks which virtual register is at each stack position during bytecode
//! emission, and emits DupN instructions to move values to the top when needed.

use super::super::types::{Label, Reg};
use crate::compiler::bytecode::Instruction;

// These methods live on Emitter because they mutate emitter state
// (both bytecode and the simulated stack).  They are separated here
// to keep the instruction-dispatch table in mod.rs focused on
// instruction encoding.
//
// Coupled fields on Emitter: `stack`, `reg_to_stack` and `bytecode`, with
// `live_in` read by the edge trim.

impl super::Emitter {
    pub(super) fn push_reg(&mut self, reg: Reg) {
        let pos = self.stack.len();
        self.stack.push(reg);
        self.reg_to_stack.insert(reg, pos);
    }

    pub(super) fn pop(&mut self) {
        if let Some(reg) = self.stack.pop() {
            self.reg_to_stack.remove(&reg);
        }
    }

    pub(super) fn ensure_on_top(&mut self, reg: Reg) {
        if let Some(&pos) = self.reg_to_stack.get(&reg) {
            let stack_top = self.stack.len().saturating_sub(1);
            if pos != stack_top {
                // Value is not on top - duplicate it to the top using DupN
                let offset = stack_top - pos;
                self.bytecode.emit(Instruction::DupN);
                self.bytecode.emit_byte(offset as u8);
                // Track the duplicated value
                self.stack.push(reg);
                // Update reg_to_stack to point to the new top position
                self.reg_to_stack.insert(reg, self.stack.len() - 1);
            }
            // else: already on top, nothing to do
        } else {
            // Register not tracked. Consuming a DupN copy drops the register's
            // entry, though its original cell may still be on the stack, so a
            // later read of the register lands here. Assume the value is
            // already on top of the stack.
        }
    }

    /// Pop the trailing dead cells from the operand stack, but never below
    /// `floor` entries. A dead cell is an orphan whose register is not live
    /// into `target`, so no path from the edge's target reads it.
    ///
    /// An orphan is a cell `reg_to_stack` does not name, left behind when
    /// `ensure_on_top` uses DupN to copy a value to the top and the copy is
    /// then consumed. The original can still hold a value a later block reads,
    /// so `live_in` decides which orphans are dead. The trim stops at a cell the
    /// map names, read or not: after a `TailCall` the simulation still names
    /// the callee and its arguments, where a native callee left one result.
    ///
    /// When control flow branches, one path may leave a dead cell another
    /// path does not, and the paths reach their merge at different depths.
    /// DupN offsets in the merge are then wrong, which shifts operands and
    /// produces incorrect results (for example a struct value in a hash-key
    /// slot → "struct keys must be immutable (got struct)").
    ///
    /// This is the FORWARD edge's trim, and it is deliberately partial: it
    /// stops at the first cell that is not dead, leaving whatever the edge is
    /// carrying to the merge. A back edge carries nothing and takes
    /// [`Self::pop_to`].
    ///
    /// `floor` is the operand depth the target block has already been fixed
    /// at by an earlier predecessor (`Emitter::edge_depth`), and trimming past
    /// it is what turns this cleanup into a miscompilation. The merge's
    /// successors inherited the simulation of the edge that fixed the depth,
    /// so they pop the dead cell a second time on a path where it is already
    /// gone. The stack falls one slot into the frame's reserved local region
    /// and the topmost local stops existing (src/lir/AGENTS.md). Pass `0`
    /// only when the target's depth is not fixed yet: this edge is then the
    /// one that fixes it.
    pub(super) fn pop_trailing_dead_to(&mut self, floor: usize, target: Label) {
        while self.stack.len() > floor {
            let top = self.stack.len() - 1;
            let reg = self.stack[top];
            let named = self.reg_to_stack.get(&reg) == Some(&top);
            let read = self
                .live_in
                .get(&target)
                .is_some_and(|live| live.contains(reg));
            if named || read {
                break;
            }
            // An orphan owns no `reg_to_stack` entry, so only the cell goes.
            self.bytecode.emit(Instruction::Pop);
            self.stack.pop();
        }
    }

    /// Emit `Pop` until the operand stack is exactly `floor` deep, whatever the
    /// cells above it are.
    ///
    /// The trim a BACK edge takes (src/lir/AGENTS.md).
    /// `pop_trailing_dead_to` stops at the first cell that is not dead, so a
    /// dead cell under such a cell survives it — and a loop body that reaches its
    /// back edge with such a pair deepens the header's stack once per trip, for
    /// as long as the loop runs. A back edge may go further than the dead-cell
    /// trim because its target was emitted before the edge existed: the
    /// target's simulation names only cells below `floor`, so it reads nothing
    /// this block pushed and every surplus cell is dead.
    pub(super) fn pop_to(&mut self, floor: usize) {
        while self.stack.len() > floor {
            self.bytecode.emit(Instruction::Pop);
            let top = self.stack.len() - 1;
            let reg = self.stack.pop().expect("len checked above");
            // Only a canonical entry owns its `reg_to_stack` record; an orphan
            // names a position some other cell holds, which must stay.
            if self.reg_to_stack.get(&reg) == Some(&top) {
                self.reg_to_stack.remove(&reg);
            }
        }
    }

    /// Ensure `regs` are the top elements of the stack, in order, the first
    /// deepest.
    ///
    /// An instruction that consumes several operands (a call, a closure, a
    /// binary operation) takes them this way. Unlike `ensure_on_top` alone,
    /// which duplicates via DupN (leaving originals as orphans), this checks
    /// whether the operands are already in position and only falls back to
    /// DupN when they aren't.
    pub(super) fn ensure_all_on_top(&mut self, regs: &[Reg]) {
        if let Some(base) = self.stack.len().checked_sub(regs.len()) {
            let in_place = regs
                .iter()
                .enumerate()
                .all(|(i, reg)| self.reg_to_stack.get(reg) == Some(&(base + i)));
            if in_place {
                return;
            }
        }
        // Fall back to ensure_on_top for each operand, which leaves the
        // original cell of each copied operand behind as an orphan.
        for &reg in regs {
            self.ensure_on_top(reg);
        }
    }

    /// Ensure two registers are the top two stack elements (lhs below rhs).
    pub(super) fn ensure_binary_on_top(&mut self, lhs: Reg, rhs: Reg) {
        self.ensure_all_on_top(&[lhs, rhs]);
    }
}
