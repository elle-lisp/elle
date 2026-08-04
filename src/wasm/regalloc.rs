//! Register allocation for the WASM emitter.
//!
//! LIR uses SSA-style virtual registers (one def per register, unlimited count).
//! Each virtual register needs a pair of WASM locals (tag + payload), so a naive
//! one-pair-per-register mapping would explode the local count.
//!
//! This module computes a mapping from virtual registers to a smaller set of
//! reusable WASM local pairs. Registers whose entire lifetime is within a single
//! basic block share locals from a pool. Cross-block registers get dedicated slots.

use crate::lir::{for_each_def, for_each_terminator_use, for_each_use, Label, LirFunction, Reg};
use std::collections::{HashMap, HashSet};

/// Result of register allocation: maps each LIR Reg to a WASM local "slot"
/// (a logical index into the compacted local array).
pub struct RegAlloc {
    /// Reg → slot index. The emitter uses `offset + slot` for the tag local
    /// and `offset + max_slots + slot` for the payload local.
    pub reg_to_slot: HashMap<Reg, u32>,
    /// Total number of slots needed (determines WASM local declarations).
    pub max_slots: u32,
}

/// Compute a register allocation for a LIR function.
///
/// Strategy:
/// 1. Find which block defines each register and which blocks use it.
/// 2. "Cross-block" registers (used outside their defining block) get
///    dedicated slots that are never reused.
/// 3. "Within-block" registers (def + all uses in one block) share slots
///    from a pool, allocated per-block with greedy reuse.
///    `pinned_regs`: registers that must have dedicated (non-reused) slots.
///    For the entry function, this is 0..num_locals because LoadLocal/StoreLocal
///    maps slot N to Reg(N) via copy_reg, requiring a stable physical mapping.
pub fn allocate(func: &LirFunction, pinned_regs: u32) -> RegAlloc {
    if func.blocks.is_empty() || func.num_regs == 0 {
        return RegAlloc {
            reg_to_slot: HashMap::new(),
            max_slots: 0,
        };
    }

    // Phase 1: compute def-block and use-blocks for each register.
    let mut def_block: HashMap<Reg, Label> = HashMap::new();
    let mut use_blocks: HashMap<Reg, HashSet<Label>> = HashMap::new();

    for block in &func.blocks {
        for si in &block.instructions {
            for_each_def(&si.instr, |reg| {
                def_block.insert(reg, block.label);
            });
            for_each_use(&si.instr, |reg| {
                use_blocks.entry(reg).or_default().insert(block.label);
            });
        }
        for_each_terminator_use(&block.terminator.terminator, |reg| {
            use_blocks.entry(reg).or_default().insert(block.label);
        });
    }

    // Phase 2: classify registers.
    let mut cross_block_regs: Vec<Reg> = Vec::new();
    // Per-block list of within-block registers, in instruction order.
    let mut block_local_regs: HashMap<Label, Vec<Reg>> = HashMap::new();

    for reg_id in 0..func.num_regs {
        let reg = Reg(reg_id);
        let def_lbl = match def_block.get(&reg) {
            Some(l) => *l,
            None => {
                // Used but never defined (e.g., function parameters, resume values).
                // Treat as cross-block since they're live-in to the function.
                if use_blocks.contains_key(&reg) {
                    cross_block_regs.push(reg);
                }
                continue;
            }
        };
        let uses = use_blocks.get(&reg);
        let is_cross_block = match uses {
            None => false, // defined but never used; within-block (still need a slot)
            Some(set) => {
                // Cross-block if used in any block other than the defining one
                set.iter().any(|l| *l != def_lbl)
            }
        };
        // Pinned registers must get dedicated slots regardless of liveness.
        if reg_id < pinned_regs || is_cross_block {
            cross_block_regs.push(reg);
        } else {
            block_local_regs.entry(def_lbl).or_default().push(reg);
        }
    }

    let mut reg_to_slot: HashMap<Reg, u32> = HashMap::new();
    let mut next_slot: u32 = 0;

    // Assign dedicated slots to cross-block registers.
    for reg in &cross_block_regs {
        reg_to_slot.insert(*reg, next_slot);
        next_slot += 1;
    }

    let cross_block_count = next_slot;

    // Phase 3: within each block, do greedy linear-scan allocation from a pool.
    // The pool slots start at `cross_block_count` and are reused across blocks.
    let mut pool_high_water: u32 = 0;

    for block in &func.blocks {
        let locals = match block_local_regs.get(&block.label) {
            Some(v) => v,
            None => continue,
        };
        if locals.is_empty() {
            continue;
        }

        // Compute last-use instruction index for each within-block register.
        let mut last_use: HashMap<Reg, usize> = HashMap::new();
        let local_set: HashSet<Reg> = locals.iter().copied().collect();

        for (idx, si) in block.instructions.iter().enumerate() {
            for_each_use(&si.instr, |reg| {
                if local_set.contains(&reg) {
                    last_use.insert(reg, idx);
                }
            });
        }
        // Check terminator uses too — encode as idx = instructions.len()
        let term_idx = block.instructions.len();
        for_each_terminator_use(&block.terminator.terminator, |reg| {
            if local_set.contains(&reg) {
                last_use.insert(reg, term_idx);
            }
        });

        // Walk instructions, allocate on def, free after last use.
        let mut free_pool: Vec<u32> = Vec::new();
        let mut active: HashMap<Reg, u32> = HashMap::new(); // reg → pool slot

        for (idx, si) in block.instructions.iter().enumerate() {
            // Allocate for defs in this instruction.
            for_each_def(&si.instr, |reg| {
                if local_set.contains(&reg) {
                    let slot = free_pool.pop().unwrap_or_else(|| {
                        let s = pool_high_water;
                        pool_high_water += 1;
                        s
                    });
                    reg_to_slot.insert(reg, cross_block_count + slot);
                    active.insert(reg, slot);
                }
            });

            // Free registers whose last use is this instruction.
            // Sort by slot to ensure deterministic free_pool ordering.
            let mut to_free = Vec::new();
            for (reg, slot) in &active {
                if last_use.get(reg).copied() == Some(idx) {
                    to_free.push((*reg, *slot));
                }
            }
            to_free.sort_by_key(|(_, slot)| *slot);
            for (reg, slot) in to_free {
                active.remove(&reg);
                free_pool.push(slot);
            }
        }

        // Free registers whose last use is the terminator.
        let mut term_free: Vec<u32> = active
            .iter()
            .filter(|(reg, _)| last_use.get(reg).copied() == Some(term_idx))
            .map(|(_, slot)| *slot)
            .collect();
        term_free.sort();
        for slot in term_free {
            free_pool.push(slot);
        }
        // Remaining active registers that have NO uses at all still got a slot
        // during def — that's fine, they'll be freed implicitly.
    }

    let max_slots = cross_block_count + pool_high_water;

    // Debug: check for any registers in 0..num_regs not in the map
    if crate::config::get().has_trace("wasm") {
        for reg_id in 0..func.num_regs {
            if !reg_to_slot.contains_key(&Reg(reg_id)) {
                eprintln!(
                    "[regalloc] DEBUG: Reg({}) has no slot (defined={}, used={})",
                    reg_id,
                    def_block.contains_key(&Reg(reg_id)),
                    use_blocks.contains_key(&Reg(reg_id)),
                );
            }
        }
    }

    RegAlloc {
        reg_to_slot,
        max_slots,
    }
}

#[cfg(test)]
mod tests;
