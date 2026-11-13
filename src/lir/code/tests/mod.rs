// audited: 2026-11-13
//! What freezing keeps: every opcode field by field, the records' layout, the pool, the header, the sites.
//!
//! docs/impl/lir.md
//!
//! `exemplar` builds one instruction per opcode and `regs` names the registers
//! each one reads and writes, both by exhaustive match, so a new opcode is
//! named by the compiler here before any test can pass over it.

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::testkit::LirFixture;
use crate::lir::{BinOp, ClosureId, CmpOp, ConvOp, OperandProof, Reg, Terminator, UnaryOp};
use crate::value::fiber::SignalBits;
use crate::value::{Arity, ConstTemplate, SymbolId, Value};

mod gpu;
mod header;
mod records;
mod regs;
mod roundtrip;

fn r(n: u32) -> Reg {
    Reg(n)
}

fn slot(n: u32) -> StaticRegion {
    StaticRegion::new(n).expect("a test slot is nonzero")
}

/// `template` as the bytes `ConstTemplate::encode` writes, for as long as the
/// test runs.
pub(super) fn template_bytes(template: &ConstTemplate) -> TemplateBytes<'static> {
    let mut bytes = Vec::new();
    template.encode(&mut bytes);
    TemplateBytes::new(Box::leak(bytes.into_boxed_slice()))
}

/// The moves the `Call` and `SuspendingCall` exemplars offer.
static MOVES: [u32; 2] = [
    Offer {
        position: 0,
        slot: 6,
    }
    .word(),
    Offer {
        position: 2,
        slot: 300,
    }
    .word(),
];

/// The late mints the `TailCall` exemplar names, at its stash slots.
static LATE_MINTS: [u32; 1] = [Offer {
    position: 1,
    slot: 9,
}
.word()];

/// The keys the `StructRest` exemplar excludes.
static EXCLUDED: [ConstRec; 2] = [
    ConstRec::immediate(ConstRef::Keyword(0xbeef)),
    ConstRec::immediate(ConstRef::Symbol(SymbolId::of("b-key"))),
];

/// One instruction of `op`'s variant, every field distinct from every other,
/// so a decoder that swaps two fields reads back a different instruction.
pub(super) fn exemplar(op: Op) -> InstrRef<'static> {
    use InstrRef as I;
    /// The variant whose only fields are a result and one source.
    macro_rules! dst_src {
        ($variant:ident) => {
            I::$variant {
                dst: r(1),
                src: r(2),
            }
        };
    }
    match op {
        Op::Const => I::Const {
            dst: r(1),
            value: ConstRef::Int(-7),
        },
        Op::ValueConst => I::ValueConst {
            dst: r(1),
            value: Value::int(77),
        },
        Op::MaterializeConst => I::MaterializeConst {
            dst: r(1),
            template: template_bytes(&ConstTemplate::Pair(
                Box::new(ConstTemplate::Symbol("a".into())),
                Box::new(ConstTemplate::Pair(
                    Box::new(ConstTemplate::String("b".into())),
                    Box::new(ConstTemplate::EmptyList),
                )),
            )),
            region: slot(21),
        },
        Op::LoadLocal => I::LoadLocal { dst: r(1), slot: 5 },
        Op::StoreLocal => I::StoreLocal { slot: 5, src: r(2) },
        Op::StoreLocalRefcounted => I::StoreLocalRefcounted { slot: 6, src: r(2) },
        Op::LoadCapture => I::LoadCapture {
            dst: r(1),
            index: 7,
        },
        Op::LoadCaptureRaw => I::LoadCaptureRaw {
            dst: r(1),
            index: 8,
        },
        Op::StoreCapture => I::StoreCapture {
            index: 9,
            src: r(2),
        },
        Op::MakeClosure => I::MakeClosure {
            dst: r(1),
            closure_id: ClosureId(4),
            captures: &[Reg(2), Reg(3), Reg(4)],
            region: slot(22),
        },
        Op::LoadSelf => I::LoadSelf { dst: r(1) },
        Op::Call => I::Call {
            dst: r(1),
            func: r(2),
            args: &[Reg(3), Reg(4), Reg(5)],
            arity_checked: true,
            region: slot(23),
            moves: Offers::new(&MOVES),
        },
        Op::SuspendingCall => I::SuspendingCall {
            dst: r(1),
            func: r(2),
            args: &[Reg(3)],
            arity_checked: false,
            region: slot(24),
            moves: Offers::new(&MOVES[..1]),
        },
        Op::TailCall => I::TailCall {
            dst: r(1),
            func: r(2),
            args: &[Reg(3), Reg(4), Reg(5)],
            arity_checked: true,
            region: slot(25),
            defer_callee_release: true,
            deferred_release_slot: Some(slot(31)),
            borrowed_arg_slots: Slots::new(&[7, 9]),
            late_mints: Offers::new(&LATE_MINTS),
        },
        Op::List => I::List {
            dst: r(1),
            head: r(2),
            tail: r(3),
            region: slot(26),
        },
        Op::MakeArrayMut => I::MakeArrayMut {
            dst: r(1),
            elements: &[Reg(2), Reg(3), Reg(4), Reg(5)],
            region: slot(27),
        },
        Op::First => I::First {
            dst: r(1),
            pair: r(2),
        },
        Op::Rest => I::Rest {
            dst: r(1),
            pair: r(2),
        },
        Op::BinOp => I::binop_proved(r(1), BinOp::Shr, r(2), r(3), OperandProof::Int),
        Op::UnaryOp => I::unary_proved(r(1), UnaryOp::BitNot, r(2), OperandProof::Int),
        Op::Convert => I::Convert {
            dst: r(1),
            op: ConvOp::FloatToInt,
            src: r(2),
        },
        Op::Compare => I::compare_proved(r(1), CmpOp::Ge, r(2), r(3), OperandProof::Int),
        Op::IsNil => dst_src!(IsNil),
        Op::IsPair => dst_src!(IsPair),
        Op::IsArray => dst_src!(IsArray),
        Op::IsArrayMut => dst_src!(IsArrayMut),
        Op::IsStruct => dst_src!(IsStruct),
        Op::IsStructMut => dst_src!(IsStructMut),
        Op::IsSet => dst_src!(IsSet),
        Op::IsSetMut => dst_src!(IsSetMut),
        Op::ArrayMutLen => dst_src!(ArrayMutLen),
        Op::MakeCaptureCell => I::MakeCaptureCell {
            dst: r(1),
            value: r(2),
            region: slot(28),
            name: SymbolId::of("a-cell"),
            mutated: true,
        },
        Op::LoadCaptureCell => I::LoadCaptureCell {
            dst: r(1),
            cell: r(2),
        },
        Op::StoreCaptureCell => I::StoreCaptureCell {
            cell: r(2),
            value: r(3),
        },
        Op::MatchFail => dst_src!(MatchFail),
        Op::FirstDestructure => dst_src!(FirstDestructure),
        Op::RestDestructure => dst_src!(RestDestructure),
        Op::ArrayMutRefDestructure => I::ArrayMutRefDestructure {
            dst: r(1),
            src: r(2),
            index: 11,
        },
        Op::ArrayMutSliceFrom => I::ArrayMutSliceFrom {
            dst: r(1),
            src: r(2),
            index: 12,
        },
        Op::StructGetOrNil => I::StructGetOrNil {
            dst: r(1),
            src: r(2),
            key: ConstRef::Keyword(0x5eed),
        },
        Op::StructGetDestructure => I::StructGetDestructure {
            dst: r(1),
            src: r(2),
            key: ConstRef::Symbol(SymbolId::of("a-key")),
        },
        Op::StructRest => I::StructRest {
            dst: r(1),
            src: r(2),
            exclude_keys: ConstList::new(&EXCLUDED),
        },
        Op::FirstOrNil => dst_src!(FirstOrNil),
        Op::RestOrNil => dst_src!(RestOrNil),
        Op::ArrayMutRefOrNil => I::ArrayMutRefOrNil {
            dst: r(1),
            src: r(2),
            index: 13,
        },
        Op::LoadResumeValue => I::LoadResumeValue { dst: r(1) },
        Op::Eval => I::Eval {
            dst: r(1),
            expr: r(2),
            env: r(3),
        },
        Op::ArrayMutExtend => I::ArrayMutExtend {
            dst: r(1),
            array: r(2),
            source: r(3),
        },
        Op::ArrayMutPush => I::ArrayMutPush {
            dst: r(1),
            array: r(2),
            value: r(3),
        },
        Op::CallArrayMut => I::CallArrayMut {
            dst: r(1),
            func: r(2),
            args: r(3),
            region: slot(29),
            args_region: slot(30),
        },
        Op::TailCallArrayMut => I::TailCallArrayMut {
            func: r(2),
            args: r(3),
            region: slot(32),
            args_region: slot(33),
        },
        Op::IncrefRegion => I::IncrefRegion {
            region_id: slot(34),
        },
        Op::DecrefRegion => I::DecrefRegion {
            region_id: slot(35),
        },
        Op::DecrefValueRegion => I::DecrefValueRegion { src: r(2) },
        Op::DecrefCellRegion => I::DecrefCellRegion { src: r(2) },
        Op::IncrefValueRegion => I::IncrefValueRegion { src: r(2) },
        Op::AdoptRegion => I::AdoptRegion {
            parent: r(2),
            child: r(3),
        },
        Op::AdoptCellRegion => I::AdoptCellRegion {
            parent: r(2),
            child: r(3),
        },
        Op::FreeRegionGroup => I::FreeRegionGroup {
            members: &[Reg(2), Reg(3), Reg(4)],
        },
        Op::AdoptIntoActivation => I::AdoptIntoActivation { child: r(2) },
        Op::AssertRegionMatches => I::AssertRegionMatches {
            region_id: slot(36),
            src: r(2),
        },
        Op::PushParamFrame => I::PushParamFrame {
            pairs: &[Reg(2), Reg(3), Reg(4), Reg(5)],
        },
        Op::PopParamFrame => I::PopParamFrame,
        Op::CheckSignalBound => I::CheckSignalBound {
            src: r(2),
            allowed_bits: SignalBits::new(0x8000_0000_0000_0011),
        },
        Op::IsEmpty => dst_src!(IsEmpty),
        Op::IsBool => dst_src!(IsBool),
        Op::IsInt => dst_src!(IsInt),
        Op::IsFloat => dst_src!(IsFloat),
        Op::IsString => dst_src!(IsString),
        Op::IsKeyword => dst_src!(IsKeyword),
        Op::IsSymbolCheck => dst_src!(IsSymbolCheck),
        Op::IsBytes => dst_src!(IsBytes),
        Op::IsBox => dst_src!(IsBox),
        Op::IsClosure => dst_src!(IsClosure),
        Op::IsFiber => dst_src!(IsFiber),
        Op::TypeOf => dst_src!(TypeOf),
        Op::Length => dst_src!(Length),
        Op::Get => I::Get {
            dst: r(1),
            obj: r(2),
            key: r(3),
        },
        Op::Put => I::Put {
            dst: r(1),
            obj: r(2),
            key: r(3),
            val: r(4),
        },
        Op::Del => I::Del {
            dst: r(1),
            obj: r(2),
            key: r(3),
        },
        Op::Has => I::Has {
            dst: r(1),
            obj: r(2),
            key: r(3),
        },
        Op::IntrPush => I::IntrPush {
            dst: r(1),
            array: r(2),
            value: r(3),
        },
        Op::IntrStringPush => I::IntrStringPush {
            dst: r(1),
            string: r(2),
            value: r(3),
        },
        Op::IntrBytesPush => I::IntrBytesPush {
            dst: r(1),
            bytes: r(2),
            value: r(3),
        },
        Op::Pop => dst_src!(Pop),
        Op::Freeze => I::Freeze {
            dst: r(1),
            src: r(2),
            region: slot(37),
        },
        Op::Thaw => I::Thaw {
            dst: r(1),
            src: r(2),
            region: slot(38),
        },
        Op::Identical => I::Identical {
            dst: r(1),
            lhs: r(2),
            rhs: r(3),
        },
    }
}

/// A function whose one block holds `instrs`, frozen.
pub(super) fn frozen(instrs: &[InstrRef<'_>]) -> LirOwned {
    LirFixture::new(Arity::Exact(0))
        .block(0, instrs, Terminator::Unreachable)
        .build()
}
