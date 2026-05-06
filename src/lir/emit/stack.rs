//! Stack simulation helpers for the LIR emitter.
//!
//! Tracks which virtual register is at each stack position during bytecode
//! emission, and emits DupN instructions to move values to the top when needed.

use super::super::types::Reg;
use crate::compiler::bytecode::Instruction;

// These methods live on Emitter because they mutate emitter state
// (both bytecode and the simulated stack).  They are separated here
// to keep the instruction-dispatch table in mod.rs focused on
// instruction encoding.
//
// Coupled fields on Emitter:
//   stack: Vec<Reg>
//   reg_to_stack: HashMap<Reg, usize>
//   bytecode: Bytecode

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
            // Register not tracked - this can happen after control flow merges
            // where the stack state is uncertain. Assume the value is already
            // on top of the stack (this is the case for if/and/or expressions
            // where each branch leaves its result on top).
            // This is a fallback for compatibility; ideally the LIR would
            // use phi nodes or a single result register for control flow.
        }
    }

    /// Ensure two registers are the top two stack elements (lhs below rhs).
    ///
    /// Binary operations (BinOp, Compare) consume both operands. Unlike
    /// `ensure_on_top` which duplicates via DupN (leaving originals as
    /// orphans), this checks whether the operands are already in position
    /// and only falls back to DupN when they aren't.
    /// Pop trailing orphan values from the operand stack.
    ///
    /// An orphan is a stack entry whose register's canonical position
    /// (in `reg_to_stack`) differs from its actual stack position.  These
    /// are left behind when `ensure_on_top` uses DupN to copy a value to
    /// the top — the original remains on the real VM stack but the
    /// simulated mapping now points to the copy.
    ///
    /// In straight-line code orphans are harmless: they sit below the
    /// active top and are eventually overwritten.  But when control flow
    /// branches, one branch may create orphans while another does not,
    /// leaving different stack depths at the merge point.  This causes
    /// DupN offsets in the merge block to be wrong, shifting operands and
    /// producing incorrect results (e.g. a struct value in a hash-key
    /// slot → "struct keys must be immutable (got struct)").
    ///
    /// Call this before saving stack state to `yield_stack_state` in
    /// terminators so that all predecessors of a merge block agree on
    /// the operand-stack depth.
    pub(super) fn pop_trailing_orphans(&mut self) {
        while let Some(&top_reg) = self.stack.last() {
            if self.reg_to_stack.get(&top_reg) == Some(&(self.stack.len() - 1)) {
                break; // top element is live — stop
            }
            // Top element is an orphan: emit Pop to remove it from the
            // real VM stack, and drop it from the simulated stack.
            self.bytecode.emit(Instruction::Pop);
            self.stack.pop();
            // Do not call self.pop() — the orphan's register either
            // isn't in reg_to_stack at all, or points to a different
            // (canonical) position that must not be disturbed.
        }
    }

    pub(super) fn ensure_binary_on_top(&mut self, lhs: Reg, rhs: Reg) {
        let stack_len = self.stack.len();
        if stack_len >= 2 {
            let lhs_pos = self.reg_to_stack.get(&lhs).copied();
            let rhs_pos = self.reg_to_stack.get(&rhs).copied();
            if lhs_pos == Some(stack_len - 2) && rhs_pos == Some(stack_len - 1) {
                // Already in place — nothing to emit
                return;
            }
        }
        // Fall back to ensure_on_top for each operand.
        // This handles the uncommon case (e.g., after control flow merges).
        // NOTE: The DupN fallback leaves original values as orphans on the
        // actual VM stack. This is a pre-existing limitation of ensure_on_top.
        // From intrinsic lowering, operands are always freshly lowered and
        // already in position, so this path is not reached in practice.
        self.ensure_on_top(lhs);
        self.ensure_on_top(rhs);
    }
}
