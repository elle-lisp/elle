// audited: 2026-11-27
//! The local slots each instruction names, written out by hand as the oracle `InstrRef::slot_refs` is checked against.
//!
//! src/lir/overview.md

use super::*;

/// Two captures and one parameter, so a capture index past 2 names slot
/// `index - 2`.
fn shape() -> LirOwned {
    LirFixture::new(Arity::Exact(1))
        .num_captures(2)
        .num_params(1)
        .build()
}

/// The slots an instruction names in [`shape`]'s layout, in field order.
fn expected_slots(i: &InstrRef<'_>) -> Vec<SlotRef> {
    use InstrRef as I;
    use SlotRef::{Read, Store};
    match *i {
        I::LoadLocal { slot, .. } => vec![Read(slot)],
        I::StoreLocal { slot, .. } => vec![Store(slot)],
        I::LoadCapture { index, .. }
        | I::LoadCaptureRaw { index, .. }
        | I::StoreCapture { index, .. } => {
            if index >= 3 {
                vec![Read(index - 2)]
            } else {
                vec![]
            }
        }
        I::DecrefPostponed { slot, route } => vec![Read(slot), Read(route)],
        I::Call { moves, .. } | I::SuspendingCall { moves, .. } => {
            moves.iter().map(|o| Read(o.slot())).collect()
        }
        I::TailCall {
            borrowed_arg_slots,
            late_mints,
            ..
        } => borrowed_arg_slots
            .iter()
            .map(Read)
            .chain(late_mints.iter().map(|o| Read(o.slot())))
            .collect(),
        I::Const { .. }
        | I::ValueConst { .. }
        | I::MaterializeConst { .. }
        | I::MakeClosure { .. }
        | I::LoadSelf { .. }
        | I::List { .. }
        | I::MakeArrayMut { .. }
        | I::First { .. }
        | I::Rest { .. }
        | I::BinOp { .. }
        | I::UnaryOp { .. }
        | I::Convert { .. }
        | I::Compare { .. }
        | I::IsNil { .. }
        | I::IsPair { .. }
        | I::IsArray { .. }
        | I::IsArrayMut { .. }
        | I::IsStruct { .. }
        | I::IsStructMut { .. }
        | I::IsSet { .. }
        | I::IsSetMut { .. }
        | I::ArrayMutLen { .. }
        | I::MakeCaptureCell { .. }
        | I::LoadCaptureCell { .. }
        | I::StoreCaptureCell { .. }
        | I::MatchFail { .. }
        | I::FirstDestructure { .. }
        | I::RestDestructure { .. }
        | I::ArrayMutRefDestructure { .. }
        | I::ArraySliceFrom { .. }
        | I::ArrayMutSliceFrom { .. }
        | I::StructGetOrNil { .. }
        | I::StructGetDestructure { .. }
        | I::StructRest { .. }
        | I::FirstOrNil { .. }
        | I::RestOrNil { .. }
        | I::ArrayMutRefOrNil { .. }
        | I::LoadResumeValue { .. }
        | I::Eval { .. }
        | I::ArrayMutExtend { .. }
        | I::ArrayMutPush { .. }
        | I::CallArrayMut { .. }
        | I::TailCallArrayMut { .. }
        | I::IncrefRegion { .. }
        | I::DecrefRegion { .. }
        | I::DecrefValueRegion { .. }
        | I::DecrefCellRegion { .. }
        | I::IncrefValueRegion { .. }
        | I::AdoptRegion { .. }
        | I::AdoptCellRegion { .. }
        | I::FreeRegionGroup { .. }
        | I::AdoptIntoActivation { .. }
        | I::AssertRegionMatches { .. }
        | I::PushParamFrame { .. }
        | I::PopParamFrame
        | I::CheckSignalBound { .. }
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
        | I::Identical { .. } => vec![],
    }
}

fn slots_of(i: &InstrRef<'_>, func: &LirOwned) -> Vec<SlotRef> {
    let mut named = Vec::new();
    i.slot_refs(&func.view(), |s| named.push(s));
    named
}

/// The counter-factual is a liveness that reads a local only through
/// `LoadLocal`: the release a postponed slot or a call's move still owes
/// would read nothing at the site, and a parked frame would lose the value
/// the abandoned-frame walk releases.
#[test]
fn each_instruction_names_the_slots_the_oracle_names() {
    let func = shape();
    for &op in Op::ALL {
        let instr = exemplar(op);
        assert_eq!(
            slots_of(&instr, &func),
            expected_slots(&instr),
            "{op:?} slots"
        );
    }
}

/// A capture index below the captures and the parameters names an upvalue
/// or a parameter's own variable, which no stack slot holds.
#[test]
fn a_capture_of_an_upvalue_or_a_parameter_names_no_slot() {
    let func = shape();
    for index in 0..3 {
        let load = InstrRef::LoadCapture { dst: r(1), index };
        assert_eq!(
            slots_of(&load, &func),
            Vec::<SlotRef>::new(),
            "capture index {index}"
        );
    }
    let past = InstrRef::LoadCapture {
        dst: r(1),
        index: 3,
    };
    assert_eq!(slots_of(&past, &func), vec![SlotRef::Read(1)]);
}
