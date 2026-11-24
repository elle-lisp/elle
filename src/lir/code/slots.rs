// audited: 2026-11-28
//! The local slots an instruction names, each as a read or a store, which slot liveness walks.
//!
//! src/lir/overview.md

use super::instr::InstrRef;
use super::view::LirView;

/// One local slot an instruction names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRef {
    /// The instruction reads the slot.
    Read(u16),
    /// The instruction replaces the slot's value.
    Store(u16),
}

impl InstrRef<'_> {
    /// Hand `f` each local slot this instruction names, in field order, in
    /// `func`'s environment layout (src/lir/overview.md).
    pub fn slot_refs(&self, func: &LirView<'_>, mut f: impl FnMut(SlotRef)) {
        use InstrRef as I;
        use SlotRef::{Read, Store};
        match *self {
            I::LoadLocal { slot, .. } => f(Read(slot)),
            I::StoreLocal { slot, .. } => {
                let params = func.num_params();
                let celled = slot as usize >= params
                    && func.capture_locals_mask().is_set(slot as usize - params);
                f(if celled { Read(slot) } else { Store(slot) })
            }
            I::LoadCapture { index, .. }
            | I::LoadCaptureRaw { index, .. }
            | I::StoreCapture { index, .. } => {
                let captures = func.num_captures();
                if index as usize >= captures as usize + func.num_params() {
                    f(Read(index - captures))
                }
            }
            I::DecrefPostponed { slot, route } => {
                f(Read(slot));
                f(Read(route));
            }
            I::Call { moves, .. } | I::SuspendingCall { moves, .. } => {
                moves.iter().for_each(|o| f(Read(o.slot())))
            }
            I::TailCall {
                borrowed_arg_slots,
                late_mints,
                ..
            } => {
                borrowed_arg_slots.iter().for_each(|s| f(Read(s)));
                late_mints.iter().for_each(|o| f(Read(o.slot())));
            }
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
            | I::SyntaxView { .. }
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
            | I::Identical { .. } => {}
        }
    }
}
