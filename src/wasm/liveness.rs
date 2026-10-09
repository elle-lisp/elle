// audited: 2026-10-06
// docs/impl/wasm.md
//! Liveness analysis for the WASM emitter's CPS spill optimization.
//!
//! Computes which physical register slots are live at each suspend point
//! (suspending call or yield). Only live slots need to be spilled, which
//! reduces code size from O(total_regs * suspend_points) to
//! O(live_regs * suspend_points).

use crate::lir::for_each_terminator_use;
use crate::lir::{InstrRef, Label, LirView, Reg, Terminator};
use std::collections::{HashMap, HashSet};

/// Per-suspend-point live set, keyed by `(block_idx, instr_idx)`.
/// Contains the set of live slots at that point, in a **combined slot space**:
/// register slots are `[0, num_phys_slots)`, stack-local slots are stored as
/// `num_phys_slots + local_slot`. A special key `(block_idx, usize::MAX)` is
/// used for Emit terminators.
pub type SpillLiveMap = HashMap<(usize, usize), HashSet<u32>>;

/// Compute the live slots at each suspend point in a function.
///
/// Works on the post-split, post-regalloc function. The `reg_to_slot` map
/// translates virtual LIR registers to physical WASM register slots.
///
/// Both register slots and stack-local slots (addressed by
/// LoadLocal/StoreLocal/StoreLocalRefcounted) are analyzed. Locals are encoded
/// in the returned sets as `num_phys_slots + local_slot` so a single set
/// describes everything that must be spilled at a suspend point. A local that
/// holds no value needed after the suspend (dead) is not spilled — the emitter
/// spills exactly the live set, keeping spill/restore code linear in the live
/// state rather than in `suspend_points × total_slots`.
pub fn compute_spill_liveness(
    func: &LirView<'_>,
    label_to_idx: &HashMap<Label, usize>,
    reg_to_slot: &HashMap<Reg, u32>,
    num_phys_slots: u32,
    num_stack_locals: u32,
) -> SpillLiveMap {
    let blocks: Vec<_> = func.blocks().collect();
    let n = blocks.len();
    if n == 0 {
        return HashMap::new();
    }

    // Build successors map from terminators.
    let successors: Vec<Vec<usize>> = blocks
        .iter()
        .map(|block| match &block.terminator() {
            Terminator::Jump(target) => {
                vec![label_to_idx.get(target).copied().unwrap_or(0)]
            }
            Terminator::Branch {
                then_label,
                else_label,
                ..
            } => {
                let mut s = Vec::new();
                if let Some(&idx) = label_to_idx.get(then_label) {
                    s.push(idx);
                }
                if let Some(&idx) = label_to_idx.get(else_label) {
                    s.push(idx);
                }
                s
            }
            Terminator::Emit { resume_label, .. } => {
                vec![label_to_idx.get(resume_label).copied().unwrap_or(0)]
            }
            Terminator::Return(_) | Terminator::Unreachable => vec![],
        })
        .collect();

    // Compute gen/kill sets per block in physical slot space.
    let mut gen: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut kill: Vec<HashSet<u32>> = vec![HashSet::new(); n];

    for (bi, block) in blocks.iter().enumerate() {
        // Walk instructions in reverse to compute gen (upward-exposed uses)
        // and kill (defs that reach the block boundary).
        let mut block_gen = HashSet::new();
        let mut block_kill = HashSet::new();

        // Terminator uses
        for_each_terminator_use(&block.terminator(), |reg| {
            if let Some(&slot) = reg_to_slot.get(&reg) {
                if slot < num_phys_slots && !block_kill.contains(&slot) {
                    block_gen.insert(slot);
                }
            }
        });

        // Instructions in reverse
        for node in block.nodes().rev() {
            // Defs kill before uses gen (reverse order)
            node.def().into_iter().for_each(|reg| {
                if let Some(&slot) = reg_to_slot.get(&reg) {
                    if slot < num_phys_slots {
                        block_gen.remove(&slot);
                        block_kill.insert(slot);
                    }
                }
            });
            node.uses().iter().copied().for_each(|reg| {
                if let Some(&slot) = reg_to_slot.get(&reg) {
                    if slot < num_phys_slots && !block_kill.contains(&slot) {
                        block_gen.insert(slot);
                    }
                }
            });
        }

        // Stack-local gen/kill: a forward pass, because a local can be read and
        // then reassigned within the same block (`(assign v (f v))`), which the
        // register reverse pass can't express (registers are SSA, locals are
        // not). Combined-slot encoding: local `slot` → `num_phys_slots + slot`.
        // LoadLocal reads; StoreLocal overwrites; StoreLocalRefcounted reads the
        // old value (to decref it) *then* overwrites — so it both uses and defs.
        let mut local_defined = std::collections::HashSet::new();
        for node in block.nodes() {
            let (use_slot, def_slot) = match node.instr() {
                InstrRef::LoadLocal { slot, .. } => (Some(slot), None),
                InstrRef::StoreLocal { slot, .. } => (None, Some(slot)),
                InstrRef::StoreLocalRefcounted { slot, .. } => (Some(slot), Some(slot)),
                _ => (None, None),
            };
            if let Some(slot) = use_slot {
                let s = num_phys_slots + slot as u32;
                if (slot as u32) < num_stack_locals && !local_defined.contains(&s) {
                    block_gen.insert(s);
                }
            }
            if let Some(slot) = def_slot {
                let s = num_phys_slots + slot as u32;
                if (slot as u32) < num_stack_locals {
                    local_defined.insert(s);
                    block_kill.insert(s);
                }
            }
        }

        gen[bi] = block_gen;
        kill[bi] = block_kill;
    }

    // Backward dataflow: live_in[b] = gen[b] ∪ (live_out[b] - kill[b])
    //                     live_out[b] = ∪ live_in[s] for s in successors[b]
    let mut live_in: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut live_out: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut changed = true;

    while changed {
        changed = false;
        for bi in (0..n).rev() {
            // live_out = union of live_in of successors
            let mut new_out = HashSet::new();
            for &succ in &successors[bi] {
                if succ < n {
                    for &slot in &live_in[succ] {
                        new_out.insert(slot);
                    }
                }
            }

            // live_in = gen ∪ (live_out - kill)
            let mut new_in = gen[bi].clone();
            for &slot in &new_out {
                if !kill[bi].contains(&slot) {
                    new_in.insert(slot);
                }
            }

            if new_in != live_in[bi] || new_out != live_out[bi] {
                changed = true;
                live_in[bi] = new_in;
                live_out[bi] = new_out;
            }
        }
    }

    // Now compute per-suspend-point live sets.
    // For a suspending call at (block_idx, instr_idx), the live set is
    // the set of slots live AFTER that instruction (i.e., used in the
    // continuation). We compute this by walking forward from the call
    // to the end of the block, collecting uses and subtracting defs.
    let mut result = SpillLiveMap::new();

    for (bi, block) in blocks.iter().enumerate() {
        // Check for Emit terminator
        if matches!(block.terminator(), Terminator::Emit { .. }) {
            // Live at yield = live_out of this block
            result.insert((bi, usize::MAX), live_out[bi].clone());
        }

        // Check for suspending calls
        for (ii, node) in block.nodes().enumerate() {
            let is_suspend = matches!(
                node.instr(),
                InstrRef::SuspendingCall { .. } | InstrRef::CallArrayMut { .. }
            );
            if !is_suspend {
                continue;
            }

            // Live at this call = slots that are live after this instruction.
            // Start with live_out of the block, then walk backward from the
            // end of the block to this instruction, applying gen/kill.
            let mut live = live_out[bi].clone();

            // Apply terminator uses
            for_each_terminator_use(&block.terminator(), |reg| {
                if let Some(&slot) = reg_to_slot.get(&reg) {
                    if slot < num_phys_slots {
                        live.insert(slot);
                    }
                }
            });

            // Walk instructions from end backward to ii+1
            for si2 in block.nodes().skip(ii + 1).rev() {
                si2.def().into_iter().for_each(|reg| {
                    if let Some(&slot) = reg_to_slot.get(&reg) {
                        if slot < num_phys_slots {
                            live.remove(&slot);
                        }
                    }
                });
                si2.uses().iter().copied().for_each(|reg| {
                    if let Some(&slot) = reg_to_slot.get(&reg) {
                        if slot < num_phys_slots {
                            live.insert(slot);
                        }
                    }
                });
            }

            // Also include the dst of this call itself — it's live after
            // the call (the resume will write it, and subsequent code uses it).
            match node.instr() {
                InstrRef::SuspendingCall { dst, .. } | InstrRef::CallArrayMut { dst, .. } => {
                    if let Some(&slot) = reg_to_slot.get(&dst) {
                        if slot < num_phys_slots {
                            live.insert(slot);
                        }
                    }
                }
                _ => {}
            }

            result.insert((bi, ii), live);
        }
    }

    result
}
