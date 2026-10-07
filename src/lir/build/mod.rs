// audited: 2026-10-06
//! `LirBuilder`: a compile unit's functions built an instruction at a time, and frozen as each one finishes.
//!
//! docs/impl/lir.md
//! src/lir/AGENTS.md

use super::code::{freeze, InstrRef, LirOwned};
use super::{BasicBlock, Label, LirFunction, SpannedInstr, SpannedTerminator, Terminator};
use crate::syntax::Span;
use crate::value::fiberheap::FiberHeap;
use crate::value::Arity;

mod grow;
mod working;

#[allow(unused_imports)]
pub(crate) use grow::{LirArena, RegionVec};
pub(crate) use working::instr as working_instr;

/// The header of the function under construction: what the lowerer writes
/// beside its instructions, read back by every view of the frozen function.
pub type LirHead = LirFunction;

/// One function under construction, and its open block.
struct Work {
    func: LirFunction,
    open: Option<BasicBlock>,
}

/// Builds one compilation unit's functions on `heap`.
///
/// Functions stack: `begin_function` sets the current one aside, as a lambda's
/// body is lowered while its parent's block is half built, and
/// `finish_function` freezes the top one and resumes the one beneath.
pub struct LirBuilder<'h> {
    #[allow(dead_code)]
    heap: &'h mut FiberHeap,
    stack: Vec<Work>,
}

impl<'h> LirBuilder<'h> {
    /// A builder over `heap`, with no function begun.
    pub fn new(heap: &'h mut FiberHeap) -> LirBuilder<'h> {
        LirBuilder {
            heap,
            stack: Vec::new(),
        }
    }

    fn top(&mut self) -> &mut Work {
        self.stack
            .last_mut()
            .expect("a builder with no function begun")
    }

    /// Begin a function of `arity`. The function under construction, if any,
    /// waits until this one finishes.
    pub fn begin_function(&mut self, arity: Arity) {
        self.stack.push(Work {
            func: LirFunction::new(arity),
            open: None,
        });
    }

    /// The current function's header.
    pub fn head(&mut self) -> &mut LirHead {
        &mut self.top().func
    }

    /// Open a block labelled `label`. The previous block must be finished.
    pub fn open_block(&mut self, label: Label) {
        let work = self.top();
        assert!(work.open.is_none(), "a block is already open");
        work.open = Some(BasicBlock::new(label));
    }

    /// Append `instr` to the open block.
    pub fn emit(&mut self, instr: InstrRef<'_>, span: Span) {
        let instr = working::instr(instr);
        self.top()
            .open
            .as_mut()
            .expect("an instruction with no block open")
            .instructions
            .push(SpannedInstr::new(instr, span));
    }

    /// Set how the open block exits.
    pub fn terminate(&mut self, term: Terminator, span: Span) {
        self.top()
            .open
            .as_mut()
            .expect("a terminator with no block open")
            .terminator = SpannedTerminator::new(term, span);
    }

    /// Close the open block. A block never terminated exits `Unreachable`.
    pub fn finish_block(&mut self) {
        let work = self.top();
        let block = work.open.take().expect("no block is open");
        work.func.blocks.push(block);
    }

    /// Freeze the current function and resume the one beneath it.
    pub fn finish_function(&mut self) -> Result<LirOwned, String> {
        let work = self.stack.pop().expect("no function begun");
        assert!(work.open.is_none(), "a function finished with a block open");
        freeze(&work.func)
    }
}

#[cfg(test)]
mod tests;
