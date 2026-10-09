// audited: 2026-10-06
//! Decoding a frozen node or block record into what a reader matches on.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md
//!
//! The inverse of the encoder in `src/lir/build/encode.rs`, field for field.
//! Every read indexes a slice, so a corrupt index panics here rather than
//! reading outside the function.

use super::instr::InstrRef;
use super::op::Op;
use super::operand::{ConstList, ConstRef, Slots, TemplateBytes};
use super::record::{flag, kind, term, BlockRec, Node};
use super::view::Parts;
use crate::hir::region::StaticRegion;
use crate::lir::{BinOp, ClosureId, CmpOp, ConvOp, Label, OperandProof, Reg, Terminator, UnaryOp};
use crate::value::fiber::SignalBits;
use crate::value::SymbolId;

/// The selectors in declaration order, so a selector byte indexes its operation.
const BINOPS: [BinOp; 10] = [
    BinOp::Add,
    BinOp::Sub,
    BinOp::Mul,
    BinOp::Div,
    BinOp::Rem,
    BinOp::BitAnd,
    BinOp::BitOr,
    BinOp::BitXor,
    BinOp::Shl,
    BinOp::Shr,
];
const CMPOPS: [CmpOp; 6] = [
    CmpOp::Eq,
    CmpOp::Ne,
    CmpOp::Lt,
    CmpOp::Le,
    CmpOp::Gt,
    CmpOp::Ge,
];
const UNARYOPS: [UnaryOp; 3] = [UnaryOp::Neg, UnaryOp::Not, UnaryOp::BitNot];
const CONVOPS: [ConvOp; 2] = [ConvOp::IntToFloat, ConvOp::FloatToInt];

/// A run of pool words as the registers they hold.
fn regs(words: &[u32]) -> &[Reg] {
    // SAFETY: `Reg` is `repr(transparent)` over `u32`, so a `u32` slice and a
    // `Reg` slice of one length have one layout.
    unsafe { std::slice::from_raw_parts(words.as_ptr() as *const Reg, words.len()) }
}

/// The registers `node` reads, in the order the encoder wrote them.
pub(crate) fn uses<'a>(node: &'a Node, parts: &Parts<'a>) -> &'a [Reg] {
    let n = node.n_uses as usize;
    if n <= 2 {
        &node.uses[..n]
    } else {
        let at = node.extra as usize;
        regs(&parts.pool[at..at + n])
    }
}

/// Where the pool words after `node`'s uses start.
fn scalars_at(node: &Node) -> usize {
    let n = node.n_uses as usize;
    node.extra as usize + if n > 2 { n } else { 0 }
}

fn region(node: &Node) -> StaticRegion {
    StaticRegion::new(node.region).expect("an allocating or calling node names its region")
}

fn slot(word: u32) -> StaticRegion {
    StaticRegion::new(word).expect("a region operand is nonzero")
}

fn proof(node: &Node) -> OperandProof {
    if node.flags & flag::PROOF_INT != 0 {
        OperandProof::Int
    } else {
        OperandProof::Unproven
    }
}

fn sub(node: &Node) -> usize {
    (node.flags & flag::SUB) as usize
}

/// The instruction `node` holds.
pub(crate) fn instr<'a>(node: &'a Node, parts: &Parts<'a>) -> InstrRef<'a> {
    use InstrRef as R;
    let op = Op::from_byte(node.op)
        .unwrap_or_else(|| panic!("frozen node carries opcode byte {}", node.op));
    let u = uses(node, parts);
    let dst = Reg(node.dst);
    let aux = node.aux;
    let at = scalars_at(node);
    let konst = |ix: u32| ConstRef::of(&parts.consts[ix as usize]);
    let bits = |ix: u32| parts.consts[ix as usize].bits;
    let checked = node.flags & flag::ARITY_CHECKED != 0;
    match op {
        Op::Const => R::Const {
            dst,
            value: konst(aux),
        },
        Op::ValueConst => R::ValueConst {
            dst,
            value: parts.values[aux as usize],
        },
        Op::MaterializeConst => {
            let start = aux as usize;
            let len = parts.pool[at] as usize;
            R::MaterializeConst {
                dst,
                template: TemplateBytes(&parts.data[start..start + len]),
                region: region(node),
            }
        }
        Op::LoadLocal => R::LoadLocal {
            dst,
            slot: aux as u16,
        },
        Op::StoreLocal => R::StoreLocal {
            slot: aux as u16,
            src: u[0],
        },
        Op::StoreLocalRefcounted => R::StoreLocalRefcounted {
            slot: aux as u16,
            src: u[0],
        },
        Op::LoadCapture => R::LoadCapture {
            dst,
            index: aux as u16,
        },
        Op::LoadCaptureRaw => R::LoadCaptureRaw {
            dst,
            index: aux as u16,
        },
        Op::StoreCapture => R::StoreCapture {
            index: aux as u16,
            src: u[0],
        },
        Op::MakeClosure => R::MakeClosure {
            dst,
            closure_id: ClosureId(aux),
            captures: u,
            region: region(node),
        },
        Op::LoadSelf => R::LoadSelf { dst },
        Op::Call => R::Call {
            dst,
            func: u[0],
            args: &u[1..],
            arity_checked: checked,
            region: region(node),
        },
        Op::SuspendingCall => R::SuspendingCall {
            dst,
            func: u[0],
            args: &u[1..],
            arity_checked: checked,
            region: region(node),
        },
        Op::TailCall => {
            let n_borrowed = parts.pool[at + 1] as usize;
            R::TailCall {
                dst,
                func: u[0],
                args: &u[1..],
                arity_checked: checked,
                region: region(node),
                defer_callee_release: node.flags & flag::DEFER_CALLEE != 0,
                deferred_release_slot: StaticRegion::new(parts.pool[at]),
                borrowed_arg_slots: Slots(&parts.pool[at + 2..at + 2 + n_borrowed]),
            }
        }
        Op::List => R::List {
            dst,
            head: u[0],
            tail: u[1],
            region: region(node),
        },
        Op::MakeArrayMut => R::MakeArrayMut {
            dst,
            elements: u,
            region: region(node),
        },
        Op::First => R::First { dst, pair: u[0] },
        Op::Rest => R::Rest { dst, pair: u[0] },
        Op::BinOp => R::BinOp {
            dst,
            op: BINOPS[sub(node)],
            lhs: u[0],
            rhs: u[1],
            proof: proof(node),
        },
        Op::UnaryOp => R::UnaryOp {
            dst,
            op: UNARYOPS[sub(node)],
            src: u[0],
            proof: proof(node),
        },
        Op::Convert => R::Convert {
            dst,
            op: CONVOPS[sub(node)],
            src: u[0],
        },
        Op::Compare => R::Compare {
            dst,
            op: CMPOPS[sub(node)],
            lhs: u[0],
            rhs: u[1],
            proof: proof(node),
        },
        Op::IsNil => R::IsNil { dst, src: u[0] },
        Op::IsPair => R::IsPair { dst, src: u[0] },
        Op::IsArray => R::IsArray { dst, src: u[0] },
        Op::IsArrayMut => R::IsArrayMut { dst, src: u[0] },
        Op::IsStruct => R::IsStruct { dst, src: u[0] },
        Op::IsStructMut => R::IsStructMut { dst, src: u[0] },
        Op::IsSet => R::IsSet { dst, src: u[0] },
        Op::IsSetMut => R::IsSetMut { dst, src: u[0] },
        Op::ArrayMutLen => R::ArrayMutLen { dst, src: u[0] },
        Op::MakeCaptureCell => R::MakeCaptureCell {
            dst,
            value: u[0],
            region: region(node),
            name: SymbolId(bits(aux)),
            mutated: node.flags & flag::MUTATED != 0,
        },
        Op::LoadCaptureCell => R::LoadCaptureCell { dst, cell: u[0] },
        Op::StoreCaptureCell => R::StoreCaptureCell {
            cell: u[0],
            value: u[1],
        },
        Op::MatchFail => R::MatchFail { dst, src: u[0] },
        Op::FirstDestructure => R::FirstDestructure { dst, src: u[0] },
        Op::RestDestructure => R::RestDestructure { dst, src: u[0] },
        Op::ArrayMutRefDestructure => R::ArrayMutRefDestructure {
            dst,
            src: u[0],
            index: aux as u16,
        },
        Op::ArrayMutSliceFrom => R::ArrayMutSliceFrom {
            dst,
            src: u[0],
            index: aux as u16,
        },
        Op::StructGetOrNil => R::StructGetOrNil {
            dst,
            src: u[0],
            key: konst(aux),
        },
        Op::StructGetDestructure => R::StructGetDestructure {
            dst,
            src: u[0],
            key: konst(aux),
        },
        Op::StructRest => {
            let first = aux as usize;
            let n = parts.pool[at] as usize;
            R::StructRest {
                dst,
                src: u[0],
                exclude_keys: ConstList(&parts.consts[first..first + n]),
            }
        }
        Op::FirstOrNil => R::FirstOrNil { dst, src: u[0] },
        Op::RestOrNil => R::RestOrNil { dst, src: u[0] },
        Op::ArrayMutRefOrNil => R::ArrayMutRefOrNil {
            dst,
            src: u[0],
            index: aux as u16,
        },
        Op::LoadResumeValue => R::LoadResumeValue { dst },
        Op::Eval => R::Eval {
            dst,
            expr: u[0],
            env: u[1],
        },
        Op::ArrayMutExtend => R::ArrayMutExtend {
            dst,
            array: u[0],
            source: u[1],
        },
        Op::ArrayMutPush => R::ArrayMutPush {
            dst,
            array: u[0],
            value: u[1],
        },
        Op::CallArrayMut => R::CallArrayMut {
            dst,
            func: u[0],
            args: u[1],
            region: region(node),
            args_region: slot(aux),
        },
        Op::TailCallArrayMut => R::TailCallArrayMut {
            func: u[0],
            args: u[1],
            region: region(node),
            args_region: slot(aux),
        },
        Op::IncrefRegion => R::IncrefRegion {
            region_id: slot(aux),
        },
        Op::DecrefRegion => R::DecrefRegion {
            region_id: slot(aux),
        },
        Op::DecrefValueRegion => R::DecrefValueRegion { src: u[0] },
        Op::DecrefCellRegion => R::DecrefCellRegion { src: u[0] },
        Op::IncrefValueRegion => R::IncrefValueRegion { src: u[0] },
        Op::AdoptRegion => R::AdoptRegion {
            parent: u[0],
            child: u[1],
        },
        Op::AdoptCellRegion => R::AdoptCellRegion {
            parent: u[0],
            child: u[1],
        },
        Op::FreeRegionGroup => R::FreeRegionGroup { members: u },
        Op::AdoptIntoActivation => R::AdoptIntoActivation { child: u[0] },
        Op::AssertRegionMatches => R::AssertRegionMatches {
            region_id: slot(aux),
            src: u[0],
        },
        Op::PushParamFrame => R::PushParamFrame { pairs: u },
        Op::PopParamFrame => R::PopParamFrame,
        Op::CheckSignalBound => R::CheckSignalBound {
            src: u[0],
            allowed_bits: SignalBits::new(bits(aux)),
        },
        Op::IsEmpty => R::IsEmpty { dst, src: u[0] },
        Op::IsBool => R::IsBool { dst, src: u[0] },
        Op::IsInt => R::IsInt { dst, src: u[0] },
        Op::IsFloat => R::IsFloat { dst, src: u[0] },
        Op::IsString => R::IsString { dst, src: u[0] },
        Op::IsKeyword => R::IsKeyword { dst, src: u[0] },
        Op::IsSymbolCheck => R::IsSymbolCheck { dst, src: u[0] },
        Op::IsBytes => R::IsBytes { dst, src: u[0] },
        Op::IsBox => R::IsBox { dst, src: u[0] },
        Op::IsClosure => R::IsClosure { dst, src: u[0] },
        Op::IsFiber => R::IsFiber { dst, src: u[0] },
        Op::TypeOf => R::TypeOf { dst, src: u[0] },
        Op::Length => R::Length { dst, src: u[0] },
        Op::Get => R::Get {
            dst,
            obj: u[0],
            key: u[1],
        },
        Op::Put => R::Put {
            dst,
            obj: u[0],
            key: u[1],
            val: u[2],
        },
        Op::Del => R::Del {
            dst,
            obj: u[0],
            key: u[1],
        },
        Op::Has => R::Has {
            dst,
            obj: u[0],
            key: u[1],
        },
        Op::IntrPush => R::IntrPush {
            dst,
            array: u[0],
            value: u[1],
        },
        Op::IntrStringPush => R::IntrStringPush {
            dst,
            string: u[0],
            value: u[1],
        },
        Op::IntrBytesPush => R::IntrBytesPush {
            dst,
            bytes: u[0],
            value: u[1],
        },
        Op::Pop => R::Pop { dst, src: u[0] },
        Op::Freeze => R::Freeze {
            dst,
            src: u[0],
            region: region(node),
        },
        Op::Thaw => R::Thaw {
            dst,
            src: u[0],
            region: region(node),
        },
        Op::Identical => R::Identical {
            dst,
            lhs: u[0],
            rhs: u[1],
        },
    }
}

/// How the block `rec` exits.
pub(crate) fn terminator(rec: &BlockRec, parts: &Parts<'_>) -> Terminator {
    match rec.term_op {
        term::RETURN => Terminator::Return(Reg(rec.term_a)),
        term::JUMP => Terminator::Jump(Label(rec.term_a)),
        term::BRANCH => Terminator::Branch {
            cond: Reg(rec.term_a),
            then_label: Label(rec.term_b),
            else_label: Label(rec.term_c),
        },
        term::EMIT => {
            let c = &parts.consts[rec.term_c as usize];
            debug_assert_eq!(c.kind, kind::BITS);
            Terminator::Emit {
                signal: SignalBits::new(c.bits),
                value: Reg(rec.term_a),
                resume_label: Label(rec.term_b),
            }
        }
        term::UNREACHABLE => Terminator::Unreachable,
        other => panic!("a frozen block carries terminator byte {other}"),
    }
}
