// audited: 2026-10-06
//! Encoding an `InstrRef` into a node, and a terminator into a block record, against the function's own tables.
//!
//! docs/impl/lir.md
//! src/lir/AGENTS.md
//!
//! The inverse of `code/decode.rs`, field for field; the round-trip tests in
//! `code/tests/roundtrip.rs` hold the two together. A node's uses are written
//! in the order `code/tests/regs.rs` names by hand.

use super::grow::{LirArena, RegionVec};
use crate::lir::code::{flag, term, BlockRec, ConstRec, Files, InstrRef, Node, Op, Parts};
use crate::lir::code::{NO_FILE, NO_REG};
use crate::lir::{Label, OperandProof, Reg, Terminator};
use crate::syntax::files::FileId;
use crate::syntax::Span;
use crate::value::Value;
use rustc_hash::FxHashMap;

/// One function's tables: everything a node names by index besides other
/// nodes. Each grows in the working region.
pub(super) struct Tables {
    pub(super) pool: RegionVec<u32>,
    pub(super) consts: RegionVec<ConstRec>,
    /// The `MaterializeConst` templates, as `ConstTemplate::encode` wrote them.
    pub(super) data: RegionVec<u8>,
    pub(super) values: RegionVec<Value>,
    /// The files the spans name, each once. A span's `file` field is one past
    /// its index here.
    pub(super) files: RegionVec<FileId>,
    /// Each value's index in `values`, so a value is stored once.
    value_ix: FxHashMap<(u64, u64), u32>,
    /// Why the function cannot freeze, once an instruction has refused.
    pub(super) refused: Option<String>,
}

/// How long each table was when a mark was taken.
#[derive(Clone, Copy)]
pub(super) struct TableMark {
    pool: usize,
    consts: usize,
    data: usize,
    values: usize,
    files: usize,
}

impl Tables {
    /// Empty tables in `arena`'s region, reusing `value_ix`'s room.
    pub(super) fn new(arena: LirArena, mut value_ix: FxHashMap<(u64, u64), u32>) -> Tables {
        value_ix.clear();
        Tables {
            pool: RegionVec::new(arena),
            consts: RegionVec::new(arena),
            data: RegionVec::new(arena),
            values: RegionVec::new(arena),
            files: RegionVec::new(arena),
            value_ix,
            refused: None,
        }
    }

    /// The dedup map's room, for the next function to reuse.
    pub(super) fn into_value_ix(self) -> FxHashMap<(u64, u64), u32> {
        self.value_ix
    }

    /// The slices a node decodes against. A working node names no other node,
    /// so the node table is empty.
    pub(super) fn parts(&self) -> Parts<'_> {
        Parts {
            nodes: &[],
            pool: self.pool.as_slice(),
            consts: self.consts.as_slice(),
            data: self.data.as_slice(),
            values: self.values.as_slice(),
            files: Files::Ids(self.files.as_slice()),
        }
    }

    pub(super) fn mark(&self) -> TableMark {
        TableMark {
            pool: self.pool.len(),
            consts: self.consts.len(),
            data: self.data.len(),
            values: self.values.len(),
            files: self.files.len(),
        }
    }

    /// Drop every entry made since `mark`, which only the instructions emitted
    /// since then name.
    pub(super) fn retract(&mut self, mark: TableMark) {
        for v in &self.values.as_slice()[mark.values..] {
            self.value_ix.remove(&(v.tag, v.payload));
        }
        self.pool.truncate(mark.pool);
        self.consts.truncate(mark.consts);
        self.data.truncate(mark.data);
        self.values.truncate(mark.values);
        self.files.truncate(mark.files);
    }

    /// One past the span's file's index in the file table, or `NO_FILE`.
    fn file(&mut self, span: &Span) -> u32 {
        let id = span.file_id();
        if !id.is_some() {
            return NO_FILE;
        }
        if let Some(ix) = self.files.as_slice().iter().position(|f| *f == id) {
            return ix as u32 + 1;
        }
        self.files.push(id);
        self.files.len() as u32
    }

    fn push_const(&mut self, rec: ConstRec) -> u32 {
        self.consts.push(rec);
        (self.consts.len() - 1) as u32
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
}

/// The room one encoding reuses: a node's uses, and the pool words that follow
/// them.
#[derive(Default)]
pub(super) struct Scratch {
    uses: Vec<Reg>,
    words: Vec<u32>,
}

fn proof_flag(proof: OperandProof) -> u8 {
    if proof.is_int() {
        flag::PROOF_INT
    } else {
        0
    }
}

/// `i` as a node, its operands appended to `t`.
pub(super) fn node(t: &mut Tables, s: &mut Scratch, i: InstrRef<'_>, span: Span) -> Node {
    use InstrRef as R;
    s.uses.clear();
    s.words.clear();
    let u = &mut s.uses;
    let w = &mut s.words;
    let checked = |c: bool| if c { flag::ARITY_CHECKED } else { 0 };
    // Each arm pushes the uses in field order and answers the destination, the
    // one scalar beside the registers, and the flag byte.
    let (dst, aux, flags): (Option<Reg>, u32, u8) = match i {
        R::Const { dst, value } => (Some(dst), t.push_const(ConstRec::immediate(value)), 0),
        R::ValueConst { dst, value } => (Some(dst), t.value(value), 0),
        R::MaterializeConst { dst, template, .. } => {
            let at = t.data.len() as u32;
            t.data.extend_from_slice(template.bytes());
            w.push(template.bytes().len() as u32);
            (Some(dst), at, 0)
        }
        R::LoadLocal { dst, slot } => (Some(dst), slot as u32, 0),
        R::StoreLocal { slot, src } | R::StoreLocalRefcounted { slot, src } => {
            u.push(src);
            (None, slot as u32, 0)
        }
        R::LoadCapture { dst, index } | R::LoadCaptureRaw { dst, index } => {
            (Some(dst), index as u32, 0)
        }
        R::StoreCapture { index, src } => {
            u.push(src);
            (None, index as u32, 0)
        }
        R::MakeClosure {
            dst,
            closure_id,
            captures,
            ..
        } => {
            u.extend_from_slice(captures);
            (Some(dst), closure_id.0, 0)
        }
        R::LoadSelf { dst } | R::LoadResumeValue { dst } => (Some(dst), 0, 0),
        R::Call {
            dst,
            func,
            args,
            arity_checked,
            ..
        }
        | R::SuspendingCall {
            dst,
            func,
            args,
            arity_checked,
            ..
        } => {
            u.push(func);
            u.extend_from_slice(args);
            (Some(dst), 0, checked(arity_checked))
        }
        R::TailCall {
            dst,
            func,
            args,
            arity_checked,
            defer_callee_release,
            deferred_release_slot,
            borrowed_arg_slots,
            ..
        } => {
            u.push(func);
            u.extend_from_slice(args);
            w.push(deferred_release_slot.map_or(0, |s| s.get()));
            w.push(borrowed_arg_slots.0.len() as u32);
            w.extend_from_slice(borrowed_arg_slots.0);
            let defer = if defer_callee_release {
                flag::DEFER_CALLEE
            } else {
                0
            };
            // Stored, though not a def: only the JIT's native-callee completion
            // path writes it.
            (Some(dst), 0, checked(arity_checked) | defer)
        }
        R::List {
            dst, head, tail, ..
        } => {
            u.extend_from_slice(&[head, tail]);
            (Some(dst), 0, 0)
        }
        R::MakeArrayMut { dst, elements, .. } => {
            u.extend_from_slice(elements);
            (Some(dst), 0, 0)
        }
        R::First { dst, pair } | R::Rest { dst, pair } => {
            u.push(pair);
            (Some(dst), 0, 0)
        }
        R::BinOp {
            dst,
            op,
            lhs,
            rhs,
            proof,
        } => {
            u.extend_from_slice(&[lhs, rhs]);
            (Some(dst), 0, op as u8 | proof_flag(proof))
        }
        R::Compare {
            dst,
            op,
            lhs,
            rhs,
            proof,
        } => {
            u.extend_from_slice(&[lhs, rhs]);
            (Some(dst), 0, op as u8 | proof_flag(proof))
        }
        R::UnaryOp {
            dst,
            op,
            src,
            proof,
        } => {
            u.push(src);
            (Some(dst), 0, op as u8 | proof_flag(proof))
        }
        R::Convert { dst, op, src } => {
            u.push(src);
            (Some(dst), 0, op as u8)
        }
        R::MakeCaptureCell {
            dst,
            value,
            name,
            mutated,
            ..
        } => {
            u.push(value);
            let flags = if mutated { flag::MUTATED } else { 0 };
            (Some(dst), t.push_const(ConstRec::bits(name.0)), flags)
        }
        R::LoadCaptureCell { dst, cell } => {
            u.push(cell);
            (Some(dst), 0, 0)
        }
        R::StoreCaptureCell { cell, value } => {
            u.extend_from_slice(&[cell, value]);
            (None, 0, 0)
        }
        R::ArrayMutRefDestructure { dst, src, index }
        | R::ArrayMutSliceFrom { dst, src, index }
        | R::ArrayMutRefOrNil { dst, src, index } => {
            u.push(src);
            (Some(dst), index as u32, 0)
        }
        R::StructGetOrNil { dst, src, key } | R::StructGetDestructure { dst, src, key } => {
            u.push(src);
            (Some(dst), t.push_const(ConstRec::immediate(key)), 0)
        }
        R::StructRest {
            dst,
            src,
            exclude_keys,
        } => {
            u.push(src);
            let at = t.consts.len() as u32;
            t.consts.extend_from_slice(exclude_keys.0);
            w.push(exclude_keys.0.len() as u32);
            (Some(dst), at, 0)
        }
        R::Eval { dst, expr, env } => {
            u.extend_from_slice(&[expr, env]);
            (Some(dst), 0, 0)
        }
        R::ArrayMutExtend { dst, array, source } => {
            u.extend_from_slice(&[array, source]);
            (Some(dst), 0, 0)
        }
        R::ArrayMutPush { dst, array, value }
        | R::IntrPush { dst, array, value }
        | R::IntrStringPush {
            dst,
            string: array,
            value,
        }
        | R::IntrBytesPush {
            dst,
            bytes: array,
            value,
        } => {
            u.extend_from_slice(&[array, value]);
            (Some(dst), 0, 0)
        }
        R::CallArrayMut {
            dst,
            func,
            args,
            args_region,
            ..
        } => {
            u.extend_from_slice(&[func, args]);
            (Some(dst), args_region.get(), 0)
        }
        R::TailCallArrayMut {
            func,
            args,
            args_region,
            ..
        } => {
            u.extend_from_slice(&[func, args]);
            (None, args_region.get(), 0)
        }
        R::IncrefRegion { region_id } | R::DecrefRegion { region_id } => (None, region_id.get(), 0),
        R::DecrefValueRegion { src }
        | R::DecrefCellRegion { src }
        | R::IncrefValueRegion { src }
        | R::AdoptIntoActivation { child: src } => {
            u.push(src);
            (None, 0, 0)
        }
        R::AdoptRegion { parent, child } | R::AdoptCellRegion { parent, child } => {
            u.extend_from_slice(&[parent, child]);
            (None, 0, 0)
        }
        R::FreeRegionGroup { members: regs } | R::PushParamFrame { pairs: regs } => {
            u.extend_from_slice(regs);
            (None, 0, 0)
        }
        R::AssertRegionMatches { region_id, src } => {
            u.push(src);
            (None, region_id.get(), 0)
        }
        R::PopParamFrame => (None, 0, 0),
        R::CheckSignalBound { src, allowed_bits } => {
            u.push(src);
            (None, t.push_const(ConstRec::bits(allowed_bits.raw())), 0)
        }
        R::Get { dst, obj, key } | R::Del { dst, obj, key } | R::Has { dst, obj, key } => {
            u.extend_from_slice(&[obj, key]);
            (Some(dst), 0, 0)
        }
        R::Put { dst, obj, key, val } => {
            u.extend_from_slice(&[obj, key, val]);
            (Some(dst), 0, 0)
        }
        R::Identical { dst, lhs, rhs } => {
            u.extend_from_slice(&[lhs, rhs]);
            (Some(dst), 0, 0)
        }
        R::IsNil { dst, src }
        | R::IsPair { dst, src }
        | R::IsArray { dst, src }
        | R::IsArrayMut { dst, src }
        | R::IsStruct { dst, src }
        | R::IsStructMut { dst, src }
        | R::IsSet { dst, src }
        | R::IsSetMut { dst, src }
        | R::ArrayMutLen { dst, src }
        | R::MatchFail { dst, src }
        | R::FirstDestructure { dst, src }
        | R::RestDestructure { dst, src }
        | R::FirstOrNil { dst, src }
        | R::RestOrNil { dst, src }
        | R::IsEmpty { dst, src }
        | R::IsBool { dst, src }
        | R::IsInt { dst, src }
        | R::IsFloat { dst, src }
        | R::IsString { dst, src }
        | R::IsKeyword { dst, src }
        | R::IsSymbolCheck { dst, src }
        | R::IsBytes { dst, src }
        | R::IsBox { dst, src }
        | R::IsClosure { dst, src }
        | R::IsFiber { dst, src }
        | R::TypeOf { dst, src }
        | R::Length { dst, src }
        | R::Pop { dst, src }
        | R::Freeze { dst, src, .. }
        | R::Thaw { dst, src, .. } => {
            u.push(src);
            (Some(dst), 0, 0)
        }
    };
    let n = s.uses.len();
    let n_uses = u16::try_from(n).unwrap_or_else(|_| {
        t.refused
            .get_or_insert_with(|| format!("an instruction reads {n} registers, past 65535"));
        u16::MAX
    });
    let extra = t.pool.len() as u32;
    if n > 2 {
        for r in &s.uses {
            t.pool.push(r.0);
        }
    }
    t.pool.extend_from_slice(&s.words);
    let at = |k: usize| s.uses.get(k).copied().unwrap_or(Reg(NO_REG));
    Node {
        start: span.start,
        end: span.end,
        line: span.line,
        col: span.col,
        file: t.file(&span),
        dst: dst.map_or(NO_REG, |r| r.0),
        region: i.region().map_or(0, |r| r.get()),
        aux,
        uses: [at(0), at(1)],
        extra,
        n_uses,
        op: Op::of(&i) as u8,
        flags,
    }
}

/// The record of a block labelled `label` exiting by `exit` at `span`. Its
/// run of nodes is written when the function freezes.
pub(super) fn exit(t: &mut Tables, label: Label, exit: Terminator, span: Span) -> BlockRec {
    let (term_op, a, b, c) = match exit {
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
            let at = t.push_const(ConstRec::bits(signal.raw()));
            (term::EMIT, value.0, resume_label.0, at)
        }
        Terminator::Unreachable => (term::UNREACHABLE, NO_REG, NO_REG, NO_REG),
    };
    BlockRec {
        label: label.0,
        first: 0,
        len: 0,
        term_a: a,
        term_b: b,
        term_c: c,
        start: span.start,
        end: span.end,
        line: span.line,
        col: span.col,
        file: t.file(&span),
        term_op,
        pad: [0; 3],
    }
}
