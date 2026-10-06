// audited: 2026-10-06
//! `LirView`: the one read API over a frozen function — its blocks, nodes, terminators, header, tables and sites.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md
//!
//! A view borrows slices, so a reader never learns which `Vec`s back them. The
//! slices a node needs to decode itself travel with it as `Parts`, so a block
//! or a node borrows the function's records rather than the view.

use super::instr::InstrRef;
use super::op::Op;
use super::owned::LirCode;
use super::record::{BlockRec, ConstRec, Node, SiteRec, NO_FILE, NO_REG};
use crate::hir::region::StaticRegion;
use crate::lir::{ClosureId, Label, Reg, Terminator};
use crate::signals::Signal;
use crate::syntax::files::FileId;
use crate::syntax::Span;
use crate::value::closure::MaskRef;
use crate::value::{Arity, Value};

/// The slices a node decodes against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Parts<'a> {
    pub(crate) nodes: &'a [Node],
    pub(crate) pool: &'a [u32],
    pub(crate) consts: &'a [ConstRec],
    pub(crate) data: &'a [u8],
    pub(crate) values: &'a [Value],
    pub(crate) files: &'a [FileId],
}

impl<'a> Parts<'a> {
    /// The span four fields and a file index spell.
    pub(crate) fn span(&self, start: u32, end: u32, line: u32, col: u32, file: u32) -> Span {
        let mut span = Span::new(start as usize, end as usize, line, col);
        if file != NO_FILE {
            span.set_file_id(self.files[file as usize - 1]);
        }
        span
    }
}

/// A read view over one frozen function.
#[derive(Clone, Copy, Debug)]
pub struct LirView<'a> {
    pub(crate) parts: Parts<'a>,
    code: &'a LirCode,
}

impl<'a> LirView<'a> {
    /// The view over an owned function's records and its values.
    pub(crate) fn over(code: &'a LirCode, values: &'a [Value]) -> LirView<'a> {
        LirView {
            parts: Parts {
                nodes: &code.nodes,
                pool: &code.pool,
                consts: &code.consts,
                data: &code.data,
                values,
                files: &code.files,
            },
            code,
        }
    }

    /// The records themselves, for the code module's own rewrites.
    pub(crate) fn code(&self) -> &'a LirCode {
        self.code
    }

    // ── header ─────────────────────────────────────────────────────────

    pub fn closure_id(&self) -> Option<ClosureId> {
        self.code.closure_id.map(ClosureId)
    }

    pub fn name(&self) -> Option<&'a str> {
        self.code.name.as_deref()
    }

    pub fn doc(&self) -> Option<&'a str> {
        self.code.doc.as_deref()
    }

    pub fn origin(&self) -> Option<Span> {
        self.code.origin
    }

    pub fn arity(&self) -> Arity {
        self.code.arity
    }

    pub fn entry(&self) -> Label {
        Label(self.code.entry)
    }

    pub fn num_regs(&self) -> u32 {
        self.code.num_regs
    }

    pub fn num_locals(&self) -> u16 {
        self.code.num_locals
    }

    pub fn num_captures(&self) -> u16 {
        self.code.num_captures
    }

    pub fn num_params(&self) -> usize {
        self.code.num_params as usize
    }

    pub fn num_local_params(&self) -> usize {
        self.code.num_local_params as usize
    }

    pub fn capture_params_mask(&self) -> u64 {
        self.code.capture_params_mask
    }

    pub fn capture_locals_mask(&self) -> MaskRef<'a> {
        MaskRef::new(&self.code.capture_locals)
    }

    pub fn signal(&self) -> Signal {
        self.code.signal
    }

    pub fn vararg_kind(&self) -> &'a crate::hir::VarargKind {
        &self.code.vararg_kind
    }

    pub fn rest_list_layout(&self) -> crate::value::RestListLayout {
        self.code.rest_list_layout
    }

    pub fn region_table(&self) -> &'a [StaticRegion] {
        &self.code.region_table
    }

    pub fn merged_slots(&self) -> &'a [StaticRegion] {
        &self.code.merged_slots
    }

    pub fn frame_release_slots(&self) -> &'a [u16] {
        &self.code.frame_release_slots
    }

    pub fn frame_release_regions(&self) -> &'a [StaticRegion] {
        &self.code.frame_release_regions
    }

    /// The values the `ValueConst` instructions load.
    pub fn values(&self) -> &'a [Value] {
        self.parts.values
    }

    /// The function's operand pool, for the tests that check where a node's
    /// run landed.
    #[cfg(test)]
    pub(crate) fn pool(&self) -> &'a [u32] {
        self.parts.pool
    }

    // ── blocks ─────────────────────────────────────────────────────────

    /// The blocks, in the order the lowerer appended them.
    pub fn blocks(&self) -> impl ExactSizeIterator<Item = BlockRef<'a>> + 'a {
        let parts = self.parts;
        self.code
            .blocks
            .iter()
            .map(move |rec| BlockRef { parts, rec })
    }

    /// How many blocks the function has.
    pub fn block_count(&self) -> usize {
        self.code.blocks.len()
    }

    /// The `i`th block in append order. Panics past the last.
    pub fn block(&self, i: usize) -> BlockRef<'a> {
        BlockRef {
            parts: self.parts,
            rec: &self.code.blocks[i],
        }
    }

    /// Every node of every block, in block order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeRef<'a>> + 'a {
        self.blocks().flat_map(|b| b.nodes())
    }

    // ── sites ──────────────────────────────────────────────────────────

    pub fn yield_points(&self) -> impl ExactSizeIterator<Item = SiteRef<'a>> + 'a {
        let regs: &'a [Reg] = &self.code.site_regs;
        self.code
            .yield_points
            .iter()
            .map(move |s| SiteRef::of(s, regs))
    }

    pub fn yield_point(&self, i: usize) -> Option<SiteRef<'a>> {
        let regs: &'a [Reg] = &self.code.site_regs;
        self.code.yield_points.get(i).map(|s| SiteRef::of(s, regs))
    }

    pub fn call_sites(&self) -> impl ExactSizeIterator<Item = SiteRef<'a>> + 'a {
        let regs: &'a [Reg] = &self.code.site_regs;
        self.code
            .call_sites
            .iter()
            .map(move |s| SiteRef::of(s, regs))
    }

    pub fn call_site(&self, i: usize) -> Option<SiteRef<'a>> {
        let regs: &'a [Reg] = &self.code.site_regs;
        self.code.call_sites.get(i).map(|s| SiteRef::of(s, regs))
    }

    /// Whether any instruction is a `op`.
    pub fn has_op(&self, op: Op) -> bool {
        self.parts.nodes.iter().any(|n| n.op == op as u8)
    }
}

/// One block of a view.
#[derive(Clone, Copy, Debug)]
pub struct BlockRef<'a> {
    parts: Parts<'a>,
    rec: &'a BlockRec,
}

impl<'a> BlockRef<'a> {
    pub fn label(&self) -> Label {
        Label(self.rec.label)
    }

    /// The block's instructions, in order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = NodeRef<'a>> + DoubleEndedIterator + 'a {
        let parts = self.parts;
        let first = self.rec.first as usize;
        let run = &parts.nodes[first..first + self.rec.len as usize];
        run.iter().map(move |node| NodeRef { parts, node })
    }

    /// The block's `j`th instruction. Panics past the last.
    pub fn node(&self, j: usize) -> NodeRef<'a> {
        assert!(j < self.len(), "node {j} of a {}-node block", self.len());
        NodeRef {
            parts: self.parts,
            node: &self.parts.nodes[self.rec.first as usize + j],
        }
    }

    /// The block's instructions, decoded.
    pub fn instrs(&self) -> impl ExactSizeIterator<Item = InstrRef<'a>> + 'a {
        self.nodes().map(|n| n.instr())
    }

    pub fn len(&self) -> usize {
        self.rec.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.rec.len == 0
    }

    pub fn terminator(&self) -> Terminator {
        super::decode::terminator(self.rec, &self.parts)
    }

    pub fn terminator_span(&self) -> Span {
        let r = self.rec;
        self.parts.span(r.start, r.end, r.line, r.col, r.file)
    }
}

/// One instruction of a view, before decoding.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> {
    parts: Parts<'a>,
    node: &'a Node,
}

impl<'a> NodeRef<'a> {
    /// The opcode. Panics on a byte no opcode has.
    pub fn op(&self) -> Op {
        Op::from_byte(self.node.op)
            .unwrap_or_else(|| panic!("frozen node carries opcode byte {}", self.node.op))
    }

    /// The instruction, decoded.
    pub fn instr(&self) -> InstrRef<'a> {
        super::decode::instr(self.node, &self.parts)
    }

    pub fn span(&self) -> Span {
        let n = self.node;
        self.parts.span(n.start, n.end, n.line, n.col, n.file)
    }

    /// The registers the instruction reads, once per operand position.
    pub fn uses(&self) -> &'a [Reg] {
        super::decode::uses(self.node, &self.parts)
    }

    /// The register the instruction writes. A `TailCall`'s `dst` is not one:
    /// only the JIT's native-callee completion path writes it.
    pub fn def(&self) -> Option<Reg> {
        if self.node.dst == NO_REG || self.node.op == Op::TailCall as u8 {
            None
        } else {
            Some(Reg(self.node.dst))
        }
    }

    /// The region slot an allocating or calling instruction routes to, as
    /// `LirInstr::region` answers it.
    pub fn region(&self) -> Option<StaticRegion> {
        StaticRegion::new(self.node.region)
    }

    /// The record itself, for the tests that check its layout.
    #[cfg(test)]
    pub(crate) fn record(&self) -> &'a Node {
        self.node
    }
}

/// A yield point or a call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteRef<'a> {
    /// The bytecode offset the interpreter resumes at.
    pub resume_ip: usize,
    /// The local slots below the operands.
    pub num_locals: u16,
    /// The registers on the operand stack, bottom to top.
    pub stack_regs: &'a [Reg],
}

impl<'a> SiteRef<'a> {
    fn of(rec: &SiteRec, regs: &'a [Reg]) -> SiteRef<'a> {
        let at = rec.regs as usize;
        SiteRef {
            resume_ip: rec.resume_ip as usize,
            num_locals: rec.num_locals,
            stack_regs: &regs[at..at + rec.n_regs as usize],
        }
    }
}
