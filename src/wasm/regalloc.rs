//! Register allocation for the WASM emitter.
//!
//! LIR uses SSA-style virtual registers (one def per register, unlimited count).
//! The WASM emitter previously mapped each virtual register to a dedicated pair
//! of WASM locals (tag + payload), causing massive local counts (~1744 for hello
//! world + stdlib).
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
            // Free registers whose last use is this instruction
            // (before allocating defs, so slots can be reused immediately).
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
    if crate::config::get().debug_wasm {
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

fn for_each_def(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { dst, .. }
        | LirInstr::ValueConst { dst, .. }
        | LirInstr::LoadLocal { dst, .. }
        | LirInstr::LoadCapture { dst, .. }
        | LirInstr::LoadCaptureRaw { dst, .. }
        | LirInstr::MakeClosure { dst, .. }
        | LirInstr::Call { dst, .. }
        | LirInstr::SuspendingCall { dst, .. }
        | LirInstr::CallArrayMut { dst, .. }
        | LirInstr::Cons { dst, .. }
        | LirInstr::MakeArrayMut { dst, .. }
        | LirInstr::Car { dst, .. }
        | LirInstr::Cdr { dst, .. }
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
        | LirInstr::MakeLBox { dst, .. }
        | LirInstr::LoadLBox { dst, .. }
        | LirInstr::CarDestructure { dst, .. }
        | LirInstr::CdrDestructure { dst, .. }
        | LirInstr::ArrayMutRefDestructure { dst, .. }
        | LirInstr::ArrayMutSliceFrom { dst, .. }
        | LirInstr::StructGetOrNil { dst, .. }
        | LirInstr::StructGetDestructure { dst, .. }
        | LirInstr::StructRest { dst, .. }
        | LirInstr::CarOrNil { dst, .. }
        | LirInstr::CdrOrNil { dst, .. }
        | LirInstr::ArrayMutRefOrNil { dst, .. }
        | LirInstr::LoadResumeValue { dst, .. }
        | LirInstr::Eval { dst, .. }
        | LirInstr::ArrayMutExtend { dst, .. }
        | LirInstr::ArrayMutPush { dst, .. } => f(*dst),

        LirInstr::StoreLocal { .. }
        | LirInstr::StoreCapture { .. }
        | LirInstr::StoreLBox { .. }
        | LirInstr::TailCall { .. }
        | LirInstr::TailCallArrayMut { .. }
        | LirInstr::RegionEnter
        | LirInstr::RegionExit
        | LirInstr::PushParamFrame { .. }
        | LirInstr::PopParamFrame
        | LirInstr::CheckSignalBound { .. } => {}
    }
}

fn for_each_use(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { .. } | LirInstr::ValueConst { .. } => {}
        LirInstr::LoadCapture { .. }
        | LirInstr::LoadCaptureRaw { .. }
        | LirInstr::LoadResumeValue { .. } => {}

        LirInstr::LoadLocal { .. } => {}
        LirInstr::StoreLocal { src, .. } => f(*src),
        LirInstr::StoreCapture { src, .. } => f(*src),
        LirInstr::StoreLBox { cell, value } => {
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
        LirInstr::TailCall { func, args } => {
            f(*func);
            for a in args {
                f(*a);
            }
        }
        LirInstr::CallArrayMut { func, args, .. } => {
            f(*func);
            f(*args);
        }
        LirInstr::TailCallArrayMut { func, args } => {
            f(*func);
            f(*args);
        }

        LirInstr::Cons { head, tail, .. } => {
            f(*head);
            f(*tail);
        }
        LirInstr::MakeArrayMut { elements, .. } => {
            for e in elements {
                f(*e);
            }
        }
        LirInstr::Car { pair, .. } | LirInstr::Cdr { pair, .. } => f(*pair),

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
        | LirInstr::CarDestructure { src, .. }
        | LirInstr::CdrDestructure { src, .. }
        | LirInstr::ArrayMutRefDestructure { src, .. }
        | LirInstr::ArrayMutSliceFrom { src, .. }
        | LirInstr::StructGetOrNil { src, .. }
        | LirInstr::StructGetDestructure { src, .. }
        | LirInstr::StructRest { src, .. }
        | LirInstr::CarOrNil { src, .. }
        | LirInstr::CdrOrNil { src, .. }
        | LirInstr::ArrayMutRefOrNil { src, .. } => f(*src),

        LirInstr::MakeLBox { value, .. } => f(*value),
        LirInstr::LoadLBox { cell, .. } => f(*cell),

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

        LirInstr::RegionEnter | LirInstr::RegionExit | LirInstr::PopParamFrame => {}
    }
}

fn for_each_terminator_use(term: &Terminator, mut f: impl FnMut(Reg)) {
    match term {
        Terminator::Return(reg) => f(*reg),
        Terminator::Branch { cond, .. } => f(*cond),
        Terminator::Yield { value, .. } => f(*value),
        Terminator::Jump(_) | Terminator::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::{BasicBlock, Label, LirFunction, SpannedInstr, SpannedTerminator};
    use crate::syntax::Span;
    use crate::value::Arity;

    fn mk_func(blocks: Vec<BasicBlock>, num_regs: u32) -> LirFunction {
        let mut f = LirFunction::new(Arity::Exact(0));
        f.blocks = blocks;
        f.num_regs = num_regs;
        f
    }

    fn mk_block(label: u32, instrs: Vec<LirInstr>, term: Terminator) -> BasicBlock {
        let mut b = BasicBlock::new(Label(label));
        b.instructions = instrs
            .into_iter()
            .map(|i| SpannedInstr::new(i, Span::synthetic()))
            .collect();
        b.terminator = SpannedTerminator::new(term, Span::synthetic());
        b
    }

    #[test]
    fn within_block_reuse() {
        // Two registers used only within the same block should share a slot.
        // r0 = const 1; r1 = const 2; r2 = add r0 r1; return r2
        use crate::lir::{BinOp as LirBinOp, LirConst};
        let block = mk_block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(1),
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(2),
                },
                LirInstr::BinOp {
                    dst: Reg(2),
                    op: LirBinOp::Add,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
            ],
            Terminator::Return(Reg(2)),
        );
        let func = mk_func(vec![block], 3);
        let alloc = allocate(&func, 0);

        // r2 is used in the terminator so it's still "within-block" (same block).
        // r0 and r1 are dead after the BinOp, so their slots can be reused.
        // But r2 is allocated after r0/r1 are freed.
        assert!(alloc.max_slots <= 3); // at most 3, ideally 2
                                       // r0 and r1 can share a slot with r2 since r2 is defined after they die.
                                       // Actually r0 and r1 die at idx 2 (BinOp uses them), r2 is defined at idx 2.
                                       // After freeing r0/r1, r2 can reuse one of their slots.
        assert!(alloc.max_slots <= 2);
    }

    #[test]
    fn cross_block_dedicated() {
        // r0 defined in block 0, used in block 1 → cross-block, gets dedicated slot.
        use crate::lir::LirConst;
        let b0 = mk_block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Jump(Label(1)),
        );
        let b1 = mk_block(1, vec![], Terminator::Return(Reg(0)));
        let func = mk_func(vec![b0, b1], 1);
        let alloc = allocate(&func, 0);

        assert_eq!(alloc.max_slots, 1);
        assert!(alloc.reg_to_slot.contains_key(&Reg(0)));
    }
}
