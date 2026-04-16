//! Perceus Phase 3: reuse fusion peephole pass.
//!
//! Scans basic blocks for `DropValue { slot }` immediately followed by
//! `Cons { dst, head, tail }` and fuses them into `ReuseSlotCons`.
//! The fused instruction reuses the old slab slot in-place: runs the
//! destructor, writes the new Cons, returns the same pointer.

use crate::lir::types::{LirFunction, LirInstr, SpannedInstr};

pub(super) fn reuse_fusion(func: &mut LirFunction) {
    for block in &mut func.blocks {
        let mut i = 0;
        while i + 1 < block.instructions.len() {
            let fuse = match (
                &block.instructions[i].instr,
                &block.instructions[i + 1].instr,
            ) {
                (LirInstr::DropValue { slot }, LirInstr::Cons { dst, head, tail }) => {
                    Some((*slot, *dst, *head, *tail))
                }
                _ => None,
            };
            if let Some((slot, dst, head, tail)) = fuse {
                let span = block.instructions[i].span.clone();
                block.instructions[i] = SpannedInstr::new(
                    LirInstr::ReuseSlotCons {
                        dst,
                        slot,
                        head,
                        tail,
                    },
                    span,
                );
                block.instructions.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }
}
