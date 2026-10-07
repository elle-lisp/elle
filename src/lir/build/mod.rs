// audited: 2026-10-06
//! `LirBuilder`: a compile unit's functions built an instruction at a time in a working region, and frozen as each one finishes.
//!
//! docs/impl/lir.md
//! src/lir/AGENTS.md

use std::marker::PhantomData;

use super::code::{InstrRef, LirOwned, Node};
use super::{Label, Terminator};
use crate::syntax::Span;
use crate::value::fiberheap::FiberHeap;
use crate::value::Arity;
use rustc_hash::FxHashMap;

mod encode;
mod freeze;
mod grow;
mod head;

pub(crate) use grow::{LirArena, RegionVec};
pub use head::LirHead;

/// One block: its label, its nodes, and how it exits.
struct Block {
    label: Label,
    nodes: RegionVec<Node>,
    exit: Terminator,
    span: Span,
}

/// One function under construction.
struct Work {
    head: LirHead,
    /// The finished blocks, in the order they finished.
    blocks: Vec<Block>,
    open: Option<Block>,
    tables: encode::Tables,
}

impl Work {
    fn open(&self) -> &Block {
        self.open.as_ref().expect("no block is open")
    }

    fn open_mut(&mut self) -> &mut Block {
        self.open.as_mut().expect("no block is open")
    }
}

/// The Rust-side room a finished function leaves, for the next to reuse: its
/// block list and its value index.
#[derive(Default)]
struct Spare {
    blocks: Vec<Block>,
    value_ix: FxHashMap<(u64, u64), u32>,
}

/// How far the open block and the function's tables reached when the mark was
/// taken, so [`LirBuilder::retract`] can take back what came after.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    nodes: usize,
    tables: encode::TableMark,
}

/// Builds one compilation unit's functions in a working region on a heap, and
/// frees the region when dropped.
///
/// Functions stack: `begin_function` sets the current one aside, as a lambda's
/// body is lowered while its parent's block is half built, and
/// `finish_function` freezes the top one and resumes the one beneath.
pub struct LirBuilder<'h> {
    arena: LirArena,
    stack: Vec<Work>,
    spare: Vec<Spare>,
    scratch: encode::Scratch,
    /// The nodes a splice carries between two blocks.
    moving: Vec<Node>,
    heap: PhantomData<&'h mut FiberHeap>,
}

impl<'h> LirBuilder<'h> {
    /// A builder over `heap`, with a working region minted there and no
    /// function begun.
    pub fn new(heap: &'h mut FiberHeap) -> LirBuilder<'h> {
        LirBuilder {
            arena: LirArena::mint(heap),
            stack: Vec::new(),
            spare: Vec::new(),
            scratch: encode::Scratch::default(),
            moving: Vec::new(),
            heap: PhantomData,
        }
    }

    /// Free the working region and hand the heap back.
    pub fn into_heap(self) -> &'h mut FiberHeap {
        let heap = self.arena.heap();
        drop(self);
        // SAFETY: the builder was made from this `&'h mut` and is gone, so the
        // borrow is unique again.
        unsafe { &mut *heap.as_ptr() }
    }

    fn top(&self) -> &Work {
        self.stack.last().expect("a builder with no function begun")
    }

    fn top_mut(&mut self) -> &mut Work {
        self.stack
            .last_mut()
            .expect("a builder with no function begun")
    }

    /// Begin a function of `arity`. The function under construction, if any,
    /// waits until this one finishes.
    pub fn begin_function(&mut self, arity: Arity) {
        let Spare { blocks, value_ix } = self.spare.pop().unwrap_or_default();
        self.stack.push(Work {
            head: LirHead::new(arity),
            blocks,
            open: None,
            tables: encode::Tables::new(self.arena, value_ix),
        });
    }

    /// The current function's header.
    pub fn head(&mut self) -> &mut LirHead {
        &mut self.top_mut().head
    }

    /// Open a block labelled `label`. The previous block must be finished.
    pub fn open_block(&mut self, label: Label) {
        let nodes = RegionVec::new(self.arena);
        let work = self.top_mut();
        assert!(work.open.is_none(), "a block is already open");
        work.open = Some(Block {
            label,
            nodes,
            exit: Terminator::Unreachable,
            span: Span::synthetic(),
        });
    }

    /// Append `instr`, written at `span`, to the open block.
    pub fn emit(&mut self, instr: InstrRef<'_>, span: Span) {
        let work = self
            .stack
            .last_mut()
            .expect("a builder with no function begun");
        let node = encode::node(&mut work.tables, &mut self.scratch, instr, span);
        work.open_mut().nodes.push(node);
    }

    /// Set how the open block exits.
    pub fn terminate(&mut self, exit: Terminator, span: Span) {
        let open = self.top_mut().open_mut();
        open.exit = exit;
        open.span = span;
    }

    /// Close the open block. A block never terminated exits `Unreachable`.
    pub fn finish_block(&mut self) {
        let work = self.top_mut();
        let block = work.open.take().expect("no block is open");
        work.blocks.push(block);
    }

    /// Freeze the current function and resume the one beneath it.
    pub fn finish_function(&mut self) -> Result<LirOwned, String> {
        let mut work = self.stack.pop().expect("no function begun");
        assert!(work.open.is_none(), "a function finished with a block open");
        let frozen = freeze::freeze(&mut work);
        let Work {
            mut blocks, tables, ..
        } = work;
        blocks.clear();
        self.spare.push(Spare {
            blocks,
            value_ix: tables.into_value_ix(),
        });
        frozen
    }

    // ── reading back ───────────────────────────────────────────────────
    //
    // The lowerer asks questions of the nodes it has emitted, decoded against
    // the function's tables exactly as a reader decodes a frozen node.

    /// The open block's label.
    pub(crate) fn open_label(&self) -> Label {
        self.top().open().label
    }

    /// How many nodes the open block holds.
    pub(crate) fn open_len(&self) -> usize {
        self.top().open().nodes.len()
    }

    /// The open block's instructions from index `from` on.
    pub(crate) fn open_instrs(&self, from: usize) -> impl Iterator<Item = InstrRef<'_>> + '_ {
        let work = self.top();
        let parts = work.tables.parts();
        work.open().nodes.as_slice()[from..]
            .iter()
            .map(move |n| parts.instr(n))
    }

    /// How many blocks the current function has finished.
    pub(crate) fn finished_len(&self) -> usize {
        self.top().blocks.len()
    }

    /// How many nodes finished block `block` holds.
    pub(crate) fn finished_block_len(&self, block: usize) -> usize {
        self.top().blocks[block].nodes.len()
    }

    /// Finished block `block`'s instruction at `at`, if it has one there.
    pub(crate) fn finished_instr(&self, block: usize, at: usize) -> Option<InstrRef<'_>> {
        let work = self.top();
        let node = work.blocks[block].nodes.as_slice().get(at)?;
        Some(work.tables.parts().instr(node))
    }

    /// Every instruction of every finished block, in block order. A debug
    /// assertion reads it, so a release build leaves it out.
    #[cfg(debug_assertions)]
    pub(crate) fn finished_instrs(&self) -> impl Iterator<Item = InstrRef<'_>> + '_ {
        let work = self.top();
        let parts = work.tables.parts();
        work.blocks
            .iter()
            .flat_map(|b| b.nodes.as_slice())
            .map(move |n| parts.instr(n))
    }

    // ── splices ────────────────────────────────────────────────────────
    //
    // A splice moves 48-byte nodes and nothing else. Each node names its pool
    // run by offset, so the run stays where `emit` wrote it.

    /// Where the open block and the tables stand now.
    pub(crate) fn mark(&self) -> Mark {
        let work = self.top();
        Mark {
            nodes: work.open().nodes.len(),
            tables: work.tables.mark(),
        }
    }

    /// Take back everything emitted since `mark`: the open block's nodes past
    /// it, and the table entries only they name.
    pub(crate) fn retract(&mut self, mark: Mark) {
        let work = self.top_mut();
        assert!(
            work.open().nodes.len() >= mark.nodes,
            "a retract past the open block's end"
        );
        work.open_mut().nodes.truncate(mark.nodes);
        work.tables.retract(mark.tables);
    }

    /// Move the open block's nodes from `start` on to index `at` of the open
    /// block, ahead of the nodes that sat there.
    pub(crate) fn move_run(&mut self, start: usize, at: usize) {
        let nodes = self.top_mut().open_mut().nodes.as_mut_slice();
        assert!(at <= start, "a run moves back, not forward");
        let n = nodes.len() - start;
        nodes[at..].rotate_right(n);
    }

    /// Move the open block's nodes from `start` on to index `at` of finished
    /// block `block`.
    pub(crate) fn move_run_to(&mut self, start: usize, block: usize, at: usize) {
        let work = self
            .stack
            .last_mut()
            .expect("a builder with no function begun");
        let open = work.open_mut();
        self.moving.clear();
        self.moving
            .extend_from_slice(&open.nodes.as_slice()[start..]);
        open.nodes.truncate(start);
        work.blocks[block].nodes.insert_slice(at, &self.moving);
    }
}

impl Drop for LirBuilder<'_> {
    fn drop(&mut self) {
        // SAFETY: the heap outlives `'h`, and every slice grown in the region
        // belongs to this builder, which is going.
        unsafe { self.arena.free() };
    }
}

#[cfg(test)]
mod tests;
