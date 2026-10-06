// audited: 2026-10-06
//! Freezing: copying a lowered `LirFunction` into the plain records every reader reads.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md
//!
//! The encoding per variant lives here and its inverse in `decode`; the
//! round-trip tests in `tests/roundtrip.rs` hold the two together. A node's uses
//! are what `for_each_use` reports, in its order, so a node answers the
//! register question exactly as the working form does.

use super::op::Op;
use super::owned::{FrozenModule, LirCode, LirOwned};
use super::record::{kind, term, BlockRec, ConstRec, Node, NO_FILE, NO_REG};
use crate::lir::{
    for_each_def, for_each_use, BasicBlock, LirConst, LirFunction, LirInstr, LirModule, Reg,
    SpannedInstr, Terminator,
};
use crate::syntax::files::FileId;
use crate::syntax::Span;
use crate::value::Value;
use rustc_hash::FxHashMap;

/// `Node::flags`, bit by bit. The low four bits select a sub-operation; the
/// booleans sit above them.
pub(crate) mod flag {
    /// `Call`, `SuspendingCall`, `TailCall`: the compiler checked the arity.
    pub(crate) const ARITY_CHECKED: u8 = 1 << 4;
    /// `TailCall`: the runtime adopts the callee closure.
    pub(crate) const DEFER_CALLEE: u8 = 1 << 5;
    /// `BinOp`, `Compare`, `UnaryOp`: the operands are proven integers.
    pub(crate) const PROOF_INT: u8 = 1 << 4;
    /// `MakeCaptureCell`: the unit assigns the binding.
    pub(crate) const MUTATED: u8 = 1 << 4;
    /// The sub-operation selector.
    pub(crate) const SUB: u8 = 0x0f;
}

/// Freeze one function.
///
/// Refuses a `LirConst::String`, which has no frozen form: a string literal
/// lowers to `MaterializeConst`, and the bytecode pool has nowhere
/// reclaimable for a string to live.
pub fn freeze(func: &LirFunction) -> Result<LirOwned, String> {
    let mut f = Freezer::default();
    for block in &func.blocks {
        f.block(block)?;
    }
    let code = LirCode {
        nodes: f.nodes,
        blocks: f.blocks,
        pool: f.pool,
        consts: f.consts,
        data: f.data,
        n_values: f.values.len() as u32,
        files: f.files,
        yield_points: Vec::new(),
        call_sites: Vec::new(),
        site_regs: Vec::new(),
        closure_id: func.closure_id.map(|c| c.0),
        name: func.name.clone(),
        arity: func.arity,
        entry: func.entry.0,
        num_regs: func.num_regs,
        num_locals: func.num_locals,
        num_captures: func.num_captures,
        num_params: func.num_params as u32,
        num_local_params: func.num_local_params as u32,
        capture_params_mask: func.capture_params_mask,
        capture_locals: func.capture_locals_mask.words().to_vec(),
        signal: func.signal,
        vararg_kind: func.vararg_kind.clone(),
        rest_list_layout: func.rest_list_layout,
        region_table: func.region_table.clone(),
        merged_slots: ascending(&func.merged_slots, |s| s.get()),
        frame_release_slots: ascending(&func.frame_release_slots, |s| *s),
        frame_release_regions: ascending(&func.frame_release_regions, |r| r.get()),
        doc: func.doc.as_deref().map(str::to_string),
        origin: func.origin,
    };
    Ok(LirOwned {
        code,
        values: f.values,
    })
}

/// `table` sorted ascending by `key`. The merge set and the release tables are
/// recorded this way, so the payload a function's code object holds and every
/// view over the frozen function agree on order as well as content
/// (docs/impl/lir.md).
fn ascending<T: Copy, K: Ord>(table: &[T], key: impl Fn(&T) -> K) -> Vec<T> {
    let mut sorted = table.to_vec();
    sorted.sort_unstable_by_key(key);
    sorted
}

impl LirModule {
    /// Freeze the entry function and every closure.
    pub fn freeze(&self) -> Result<FrozenModule, String> {
        Ok(FrozenModule {
            entry: freeze(&self.entry)?,
            closures: self.closures.iter().map(freeze).collect::<Result<_, _>>()?,
        })
    }
}

/// The tables one function's freeze fills.
#[derive(Default)]
struct Freezer {
    nodes: Vec<Node>,
    blocks: Vec<BlockRec>,
    pool: Vec<u32>,
    consts: Vec<ConstRec>,
    data: Vec<u8>,
    files: Vec<FileId>,
    file_ix: FxHashMap<FileId, u32>,
    values: Vec<Value>,
    value_ix: FxHashMap<(u64, u64), u32>,
    uses: Vec<Reg>,
}

impl Freezer {
    fn block(&mut self, b: &BasicBlock) -> Result<(), String> {
        let first = self.nodes.len() as u32;
        for si in &b.instructions {
            let node = self.node(si)?;
            self.nodes.push(node);
        }
        let span = b.terminator.span;
        let (term_op, a, bb, c) = match &b.terminator.terminator {
            Terminator::Return(r) => (term::RETURN, r.0, NO_REG, NO_REG),
            Terminator::Jump(l) => (term::JUMP, l.0, NO_REG, NO_REG),
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => (term::BRANCH, cond.0, then_label.0, else_label.0),
            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => {
                let at = self.push_const(kind::BITS, signal.raw());
                (term::EMIT, value.0, resume_label.0, at)
            }
            Terminator::Unreachable => (term::UNREACHABLE, NO_REG, NO_REG, NO_REG),
        };
        let file = self.file(&span);
        self.blocks.push(BlockRec {
            label: b.label.0,
            first,
            len: b.instructions.len() as u32,
            term_a: a,
            term_b: bb,
            term_c: c,
            start: span.start,
            end: span.end,
            line: span.line,
            col: span.col,
            file,
            term_op,
            pad: [0; 3],
        });
        Ok(())
    }

    /// One past the span's file's index in the table, or `NO_FILE`.
    fn file(&mut self, span: &Span) -> u32 {
        let id = span.file_id();
        if !id.is_some() {
            return NO_FILE;
        }
        if let Some(&ix) = self.file_ix.get(&id) {
            return ix;
        }
        self.files.push(id);
        let ix = self.files.len() as u32;
        self.file_ix.insert(id, ix);
        ix
    }

    fn push_const(&mut self, kind: u8, bits: u64) -> u32 {
        self.consts.push(ConstRec {
            kind,
            pad: [0; 7],
            bits,
        });
        (self.consts.len() - 1) as u32
    }

    fn lir_const(&mut self, c: &LirConst) -> Result<u32, String> {
        let (k, bits) = match c {
            LirConst::Nil => (kind::NIL, 0),
            LirConst::EmptyList => (kind::EMPTY_LIST, 0),
            LirConst::Bool(b) => (kind::BOOL, *b as u64),
            LirConst::Int(n) => (kind::INT, *n as u64),
            LirConst::Float(f) => (kind::FLOAT, f.to_bits()),
            LirConst::Symbol(s) => (kind::SYMBOL, s.0),
            LirConst::Keyword(k) => (kind::KEYWORD, *k),
            LirConst::String(_) => {
                return Err(
                    "freeze: a LirConst::String has no frozen form; a string literal \
                     lowers to MaterializeConst"
                        .to_string(),
                )
            }
        };
        Ok(self.push_const(k, bits))
    }

    /// The value's index in the table, each value once.
    fn value(&mut self, v: Value) -> u32 {
        let key = (v.tag, v.payload);
        if let Some(&ix) = self.value_ix.get(&key) {
            return ix;
        }
        self.values.push(v);
        let ix = (self.values.len() - 1) as u32;
        self.value_ix.insert(key, ix);
        ix
    }

    fn node(&mut self, si: &SpannedInstr) -> Result<Node, String> {
        let i = &si.instr;
        let span = si.span;
        self.uses.clear();
        for_each_use(i, |r| self.uses.push(r));
        let n = self.uses.len();
        let mut node = Node {
            start: span.start,
            end: span.end,
            line: span.line,
            col: span.col,
            file: self.file(&span),
            dst: NO_REG,
            region: i.region().map_or(0, |r| r.get()),
            aux: 0,
            uses: [
                self.uses.first().copied().unwrap_or(Reg(NO_REG)),
                self.uses.get(1).copied().unwrap_or(Reg(NO_REG)),
            ],
            extra: self.pool.len() as u32,
            n_uses: u16::try_from(n)
                .map_err(|_| "freeze: an instruction reads more than 65535 registers")?,
            op: Op::of(i) as u8,
            flags: 0,
        };
        if n > 2 {
            let words: Vec<u32> = self.uses.iter().map(|r| r.0).collect();
            self.pool.extend_from_slice(&words);
        }
        for_each_def(i, |r| node.dst = r.0);
        self.scalars(i, &mut node)?;
        Ok(node)
    }

    /// The fields beside the registers: `dst` where the walkers skip it, `aux`,
    /// the flags, and the pool words that follow the uses.
    fn scalars(&mut self, i: &LirInstr, node: &mut Node) -> Result<(), String> {
        use LirInstr as I;
        match i {
            I::Const { value, .. } => node.aux = self.lir_const(value)?,
            I::ValueConst { value, .. } => node.aux = self.value(*value),
            I::MaterializeConst { template, .. } => {
                node.aux = self.data.len() as u32;
                template.encode(&mut self.data);
                let len = self.data.len() as u32 - node.aux;
                self.pool.push(len);
            }
            I::LoadLocal { slot, .. }
            | I::StoreLocal { slot, .. }
            | I::StoreLocalRefcounted { slot, .. } => node.aux = *slot as u32,
            I::LoadCapture { index, .. }
            | I::LoadCaptureRaw { index, .. }
            | I::StoreCapture { index, .. }
            | I::ArrayMutRefDestructure { index, .. }
            | I::ArrayMutSliceFrom { index, .. }
            | I::ArrayMutRefOrNil { index, .. } => node.aux = *index as u32,
            I::MakeClosure { closure_id, .. } => node.aux = closure_id.0,
            I::Call { arity_checked, .. } | I::SuspendingCall { arity_checked, .. } => {
                if *arity_checked {
                    node.flags |= flag::ARITY_CHECKED;
                }
            }
            I::TailCall {
                dst,
                arity_checked,
                defer_callee_release,
                deferred_release_slot,
                borrowed_arg_slots,
                ..
            } => {
                node.dst = dst.0;
                if *arity_checked {
                    node.flags |= flag::ARITY_CHECKED;
                }
                if *defer_callee_release {
                    node.flags |= flag::DEFER_CALLEE;
                }
                self.pool.push(deferred_release_slot.map_or(0, |s| s.get()));
                self.pool.push(borrowed_arg_slots.len() as u32);
                self.pool
                    .extend(borrowed_arg_slots.iter().map(|&s| s as u32));
            }
            I::BinOp { op, proof, .. } => node.flags = *op as u8 | proof_flag(proof),
            I::Compare { op, proof, .. } => node.flags = *op as u8 | proof_flag(proof),
            I::UnaryOp { op, proof, .. } => node.flags = *op as u8 | proof_flag(proof),
            I::Convert { op, .. } => node.flags = *op as u8,
            I::MakeCaptureCell { name, mutated, .. } => {
                node.aux = self.push_const(kind::BITS, name.0);
                if *mutated {
                    node.flags |= flag::MUTATED;
                }
            }
            I::StructGetOrNil { key, .. } | I::StructGetDestructure { key, .. } => {
                node.aux = self.lir_const(key)?
            }
            I::StructRest { exclude_keys, .. } => {
                node.aux = self.consts.len() as u32;
                for k in exclude_keys {
                    self.lir_const(k)?;
                }
                self.pool.push(exclude_keys.len() as u32);
            }
            I::CallArrayMut { args_region, .. } | I::TailCallArrayMut { args_region, .. } => {
                node.aux = args_region.get()
            }
            I::IncrefRegion { region_id }
            | I::DecrefRegion { region_id }
            | I::AssertRegionMatches { region_id, .. } => node.aux = region_id.get(),
            I::CheckSignalBound { allowed_bits, .. } => {
                node.aux = self.push_const(kind::BITS, allowed_bits.raw())
            }
            // The registers are the whole instruction.
            I::LoadSelf { .. }
            | I::List { .. }
            | I::MakeArrayMut { .. }
            | I::First { .. }
            | I::Rest { .. }
            | I::IsNil { .. }
            | I::IsPair { .. }
            | I::IsArray { .. }
            | I::IsArrayMut { .. }
            | I::IsStruct { .. }
            | I::IsStructMut { .. }
            | I::IsSet { .. }
            | I::IsSetMut { .. }
            | I::ArrayMutLen { .. }
            | I::LoadCaptureCell { .. }
            | I::StoreCaptureCell { .. }
            | I::MatchFail { .. }
            | I::FirstDestructure { .. }
            | I::RestDestructure { .. }
            | I::FirstOrNil { .. }
            | I::RestOrNil { .. }
            | I::LoadResumeValue { .. }
            | I::Eval { .. }
            | I::ArrayMutExtend { .. }
            | I::ArrayMutPush { .. }
            | I::DecrefValueRegion { .. }
            | I::DecrefCellRegion { .. }
            | I::IncrefValueRegion { .. }
            | I::AdoptRegion { .. }
            | I::AdoptCellRegion { .. }
            | I::FreeRegionGroup { .. }
            | I::AdoptIntoActivation { .. }
            | I::PushParamFrame { .. }
            | I::PopParamFrame
            | I::IsEmpty { .. }
            | I::IsBool { .. }
            | I::IsInt { .. }
            | I::IsFloat { .. }
            | I::IsString { .. }
            | I::IsKeyword { .. }
            | I::IsSymbolCheck { .. }
            | I::IsBytes { .. }
            | I::IsBox { .. }
            | I::IsClosure { .. }
            | I::IsFiber { .. }
            | I::TypeOf { .. }
            | I::Length { .. }
            | I::Get { .. }
            | I::Put { .. }
            | I::Del { .. }
            | I::Has { .. }
            | I::IntrPush { .. }
            | I::IntrStringPush { .. }
            | I::IntrBytesPush { .. }
            | I::Pop { .. }
            | I::Freeze { .. }
            | I::Thaw { .. }
            | I::Identical { .. } => {}
        }
        Ok(())
    }
}

fn proof_flag(proof: &crate::lir::OperandProof) -> u8 {
    if proof.is_int() {
        flag::PROOF_INT
    } else {
        0
    }
}
