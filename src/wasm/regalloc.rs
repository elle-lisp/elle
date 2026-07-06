//! Register allocation for the WASM emitter.
//!
//! LIR uses SSA-style virtual registers (one def per register, unlimited count).
//! Each virtual register needs a pair of WASM locals (tag + payload), so a naive
//! one-pair-per-register mapping would explode the local count.
//!
//! This module computes a mapping from virtual registers to a smaller set of
//! reusable WASM local pairs. Registers whose entire lifetime is within a single
//! basic block share locals from a pool. Cross-block registers get dedicated slots.

use crate::lir::{Label, LirFunction, LirInstr, Reg, Terminator};
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

// --- Helpers: extract defs and uses from instructions ---

pub fn for_each_def(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { dst, .. }
        | LirInstr::ValueConst { dst, .. }
        | LirInstr::MaterializeConst { dst, .. }
        | LirInstr::LoadLocal { dst, .. }
        | LirInstr::LoadCapture { dst, .. }
        | LirInstr::LoadCaptureRaw { dst, .. }
        | LirInstr::LoadSelf { dst, .. }
        | LirInstr::MakeClosure { dst, .. }
        | LirInstr::Call { dst, .. }
        | LirInstr::SuspendingCall { dst, .. }
        | LirInstr::CallArrayMut { dst, .. }
        | LirInstr::List { dst, .. }
        | LirInstr::MakeArrayMut { dst, .. }
        | LirInstr::First { dst, .. }
        | LirInstr::Rest { dst, .. }
        | LirInstr::BinOp { dst, .. }
        | LirInstr::UnaryOp { dst, .. }
        | LirInstr::Compare { dst, .. }
        | LirInstr::IsNil { dst, .. }
        | LirInstr::IsPair { dst, .. }
        | LirInstr::IsArray { dst, .. }
        | LirInstr::IsArrayMut { dst, .. }
        | LirInstr::IsStruct { dst, .. }
        | LirInstr::IsStructMut { dst, .. }
        | LirInstr::IsSet { dst, .. }
        | LirInstr::IsSetMut { dst, .. }
        | LirInstr::ArrayMutLen { dst, .. }
        | LirInstr::MakeCaptureCell { dst, .. }
        | LirInstr::LoadCaptureCell { dst, .. }
        | LirInstr::MatchFail { dst, .. }
        | LirInstr::FirstDestructure { dst, .. }
        | LirInstr::RestDestructure { dst, .. }
        | LirInstr::ArrayMutRefDestructure { dst, .. }
        | LirInstr::ArrayMutSliceFrom { dst, .. }
        | LirInstr::StructGetOrNil { dst, .. }
        | LirInstr::StructGetDestructure { dst, .. }
        | LirInstr::StructRest { dst, .. }
        | LirInstr::FirstOrNil { dst, .. }
        | LirInstr::RestOrNil { dst, .. }
        | LirInstr::ArrayMutRefOrNil { dst, .. }
        | LirInstr::LoadResumeValue { dst, .. }
        | LirInstr::Eval { dst, .. }
        | LirInstr::ArrayMutExtend { dst, .. }
        | LirInstr::ArrayMutPush { dst, .. }
        | LirInstr::Convert { dst, .. }
        | LirInstr::IsEmpty { dst, .. }
        | LirInstr::IsBool { dst, .. }
        | LirInstr::IsInt { dst, .. }
        | LirInstr::IsFloat { dst, .. }
        | LirInstr::IsString { dst, .. }
        | LirInstr::IsKeyword { dst, .. }
        | LirInstr::IsSymbolCheck { dst, .. }
        | LirInstr::IsBytes { dst, .. }
        | LirInstr::IsBox { dst, .. }
        | LirInstr::IsClosure { dst, .. }
        | LirInstr::IsFiber { dst, .. }
        | LirInstr::TypeOf { dst, .. }
        | LirInstr::Length { dst, .. }
        | LirInstr::Get { dst, .. }
        | LirInstr::Put { dst, .. }
        | LirInstr::Del { dst, .. }
        | LirInstr::Has { dst, .. }
        | LirInstr::Pop { dst, .. }
        | LirInstr::Freeze { dst, .. }
        | LirInstr::Thaw { dst, .. }
        | LirInstr::IntrPush { dst, .. }
        | LirInstr::IntrStringPush { dst, .. }
        | LirInstr::IntrBytesPush { dst, .. }
        | LirInstr::Identical { dst, .. } => f(*dst),

        LirInstr::StoreLocal { .. }
        | LirInstr::StoreLocalRefcounted { .. }
        | LirInstr::StoreCapture { .. }
        | LirInstr::StoreCaptureCell { .. }
        | LirInstr::TailCall { .. }
        | LirInstr::TailCallArrayMut { .. }
        | LirInstr::IncrefRegion { .. }
        | LirInstr::DecrefRegion { .. }
        | LirInstr::DecrefValueRegion { .. }
        | LirInstr::DecrefCellRegion { .. }
        | LirInstr::IncrefValueRegion { .. }
        | LirInstr::AssertRegionMatches { .. }
        | LirInstr::AdoptRegion { .. }
        | LirInstr::AdoptCellRegion { .. }
        | LirInstr::AdoptIntoActivation { .. }
        | LirInstr::FreeRegionGroup { .. }
        | LirInstr::PushParamFrame { .. }
        | LirInstr::PopParamFrame
        | LirInstr::CheckSignalBound { .. } => {}
    }
}

pub fn for_each_use(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { .. }
        | LirInstr::ValueConst { .. }
        | LirInstr::MaterializeConst { .. } => {}
        LirInstr::LoadCapture { .. }
        | LirInstr::LoadCaptureRaw { .. }
        | LirInstr::LoadSelf { .. }
        | LirInstr::LoadResumeValue { .. } => {}

        LirInstr::LoadLocal { .. } => {}
        LirInstr::StoreLocal { src, .. } => f(*src),
        LirInstr::StoreCapture { src, .. } => f(*src),
        LirInstr::StoreCaptureCell { cell, value } => {
            f(*cell);
            f(*value);
        }
        LirInstr::CheckSignalBound { src, .. } => f(*src),

        LirInstr::MakeClosure { captures, .. } => {
            for c in captures {
                f(*c);
            }
        }

        LirInstr::Call { func, args, .. } | LirInstr::SuspendingCall { func, args, .. } => {
            f(*func);
            for a in args {
                f(*a);
            }
        }
        LirInstr::TailCall { func, args, .. } => {
            f(*func);
            for a in args {
                f(*a);
            }
        }
        LirInstr::CallArrayMut { func, args, .. } => {
            f(*func);
            f(*args);
        }
        LirInstr::TailCallArrayMut { func, args, .. } => {
            f(*func);
            f(*args);
        }

        LirInstr::List { head, tail, .. } => {
            f(*head);
            f(*tail);
        }
        LirInstr::MakeArrayMut { elements, .. } => {
            for e in elements {
                f(*e);
            }
        }
        LirInstr::First { pair, .. } | LirInstr::Rest { pair, .. } => f(*pair),

        LirInstr::BinOp { lhs, rhs, .. } | LirInstr::Compare { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }
        LirInstr::UnaryOp { src, .. }
        | LirInstr::IsNil { src, .. }
        | LirInstr::IsPair { src, .. }
        | LirInstr::IsArray { src, .. }
        | LirInstr::IsArrayMut { src, .. }
        | LirInstr::IsStruct { src, .. }
        | LirInstr::IsStructMut { src, .. }
        | LirInstr::IsSet { src, .. }
        | LirInstr::IsSetMut { src, .. }
        | LirInstr::ArrayMutLen { src, .. }
        | LirInstr::MatchFail { src, .. }
        | LirInstr::FirstDestructure { src, .. }
        | LirInstr::RestDestructure { src, .. }
        | LirInstr::ArrayMutRefDestructure { src, .. }
        | LirInstr::ArrayMutSliceFrom { src, .. }
        | LirInstr::StructGetOrNil { src, .. }
        | LirInstr::StructGetDestructure { src, .. }
        | LirInstr::StructRest { src, .. }
        | LirInstr::FirstOrNil { src, .. }
        | LirInstr::RestOrNil { src, .. }
        | LirInstr::ArrayMutRefOrNil { src, .. }
        | LirInstr::Convert { src, .. }
        | LirInstr::IsEmpty { src, .. }
        | LirInstr::IsBool { src, .. }
        | LirInstr::IsInt { src, .. }
        | LirInstr::IsFloat { src, .. }
        | LirInstr::IsString { src, .. }
        | LirInstr::IsKeyword { src, .. }
        | LirInstr::IsSymbolCheck { src, .. }
        | LirInstr::IsBytes { src, .. }
        | LirInstr::IsBox { src, .. }
        | LirInstr::IsClosure { src, .. }
        | LirInstr::IsFiber { src, .. }
        | LirInstr::TypeOf { src, .. }
        | LirInstr::Length { src, .. }
        | LirInstr::Pop { src, .. }
        | LirInstr::Freeze { src, .. }
        | LirInstr::Thaw { src, .. } => f(*src),

        LirInstr::IntrPush { array, value, .. } => {
            f(*array);
            f(*value);
        }
        LirInstr::IntrStringPush { string, value, .. } => {
            f(*string);
            f(*value);
        }
        LirInstr::IntrBytesPush { bytes, value, .. } => {
            f(*bytes);
            f(*value);
        }
        LirInstr::Get { obj, key, .. }
        | LirInstr::Del { obj, key, .. }
        | LirInstr::Has { obj, key, .. } => {
            f(*obj);
            f(*key);
        }
        LirInstr::Put { obj, key, val, .. } => {
            f(*obj);
            f(*key);
            f(*val);
        }
        LirInstr::Identical { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }

        LirInstr::MakeCaptureCell { value, .. } => f(*value),
        LirInstr::LoadCaptureCell { cell, .. } => f(*cell),

        LirInstr::Eval { expr, env, .. } => {
            f(*expr);
            f(*env);
        }
        LirInstr::ArrayMutExtend { array, source, .. } => {
            f(*array);
            f(*source);
        }
        LirInstr::ArrayMutPush { array, value, .. } => {
            f(*array);
            f(*value);
        }

        LirInstr::PushParamFrame { pairs } => {
            for (param, value) in pairs {
                f(*param);
                f(*value);
            }
        }

        LirInstr::IncrefRegion { .. } | LirInstr::DecrefRegion { .. } | LirInstr::PopParamFrame => {
        }

        LirInstr::StoreLocalRefcounted { src, .. } => f(*src),
        LirInstr::DecrefValueRegion { src, .. } => f(*src),
        LirInstr::DecrefCellRegion { src } => f(*src),
        LirInstr::IncrefValueRegion { src } => f(*src),
        // The oracle peeks `src` (the return value the slot is claimed to
        // name); record the use so liveness keeps it alive across the check.
        LirInstr::AssertRegionMatches { src, .. } => f(*src),
        // The ownership-forest ops load their operand values (the handler pops
        // them to drive the adopt / group free); record those uses so liveness
        // keeps them alive even though this backend never executes the op.
        LirInstr::AdoptRegion { parent, child } | LirInstr::AdoptCellRegion { parent, child } => {
            f(*parent);
            f(*child);
        }
        LirInstr::AdoptIntoActivation { child } => f(*child),
        LirInstr::FreeRegionGroup { members } => {
            for m in members {
                f(*m);
            }
        }
    }
}

pub fn for_each_terminator_use(term: &Terminator, mut f: impl FnMut(Reg)) {
    match term {
        Terminator::Return(reg) => f(*reg),
        Terminator::Branch { cond, .. } => f(*cond),
        Terminator::Emit { value, .. } => f(*value),
        Terminator::Jump(_) | Terminator::Unreachable => {}
    }
}

#[cfg(test)]
mod tests;
