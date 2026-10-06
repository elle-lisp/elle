// audited: 2026-10-06
//! Splitting a frozen function's blocks after chosen instructions, by re-cutting block records over the same nodes.
//!
//! docs/impl/lir.md
//! docs/impl/wasm.md

use super::op::Op;
use super::owned::LirOwned;
use super::record::{term, BlockRec, NO_REG};
use super::view::LirView;

impl<'a> LirView<'a> {
    /// A copy of this function whose blocks end after every instruction `at`
    /// selects, except a block's last. Each cut block jumps to a fresh label
    /// that starts the rest; the last piece keeps the block's terminator. New
    /// labels count up from the highest label the function holds.
    ///
    /// A block is a run of nodes plus a terminator, so a split moves no node:
    /// it writes new block records over the same runs. A cut block's jump
    /// carries the original terminator's span.
    pub fn split_after(&self, at: impl Fn(Op) -> bool) -> LirOwned {
        let records = self.block_records();
        let nodes = self.parts.nodes;
        let mut next_label = records.iter().map(|b| b.label).max().unwrap_or(0) + 1;
        let mut blocks = Vec::with_capacity(records.len());
        for rec in records {
            let mut start = rec.first;
            let mut label = rec.label;
            let end = rec.first + rec.len;
            for i in rec.first..end.saturating_sub(1) {
                let op = Op::from_byte(nodes[i as usize].op).expect("a frozen node's opcode");
                if !at(op) {
                    continue;
                }
                let cont = next_label;
                next_label += 1;
                blocks.push(BlockRec {
                    label,
                    first: start,
                    len: i + 1 - start,
                    term_a: cont,
                    term_b: NO_REG,
                    term_c: NO_REG,
                    term_op: term::JUMP,
                    ..*rec
                });
                start = i + 1;
                label = cont;
            }
            blocks.push(BlockRec {
                label,
                first: start,
                len: end - start,
                ..*rec
            });
        }
        let mut split = self.to_owned();
        split.code.blocks = blocks;
        split
    }
}
