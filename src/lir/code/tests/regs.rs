// audited: 2026-10-06
//! The registers each instruction reads and writes, written out by hand as the oracle a frozen node is checked against.
//!
//! docs/impl/lir.md

use super::*;

/// The register an instruction writes. A `TailCall`'s `dst` is not one: the
/// call replaces the frame, so only the JIT's native-callee completion path
/// writes it.
pub(super) fn expected_def(i: &InstrRef<'_>) -> Option<Reg> {
    use InstrRef as I;
    match *i {
        I::Const { dst, .. }
        | I::ValueConst { dst, .. }
        | I::MaterializeConst { dst, .. }
        | I::LoadLocal { dst, .. }
        | I::LoadCapture { dst, .. }
        | I::LoadCaptureRaw { dst, .. }
        | I::LoadSelf { dst }
        | I::MakeClosure { dst, .. }
        | I::Call { dst, .. }
        | I::SuspendingCall { dst, .. }
        | I::CallArrayMut { dst, .. }
        | I::List { dst, .. }
        | I::MakeArrayMut { dst, .. }
        | I::First { dst, .. }
        | I::Rest { dst, .. }
        | I::BinOp { dst, .. }
        | I::UnaryOp { dst, .. }
        | I::Compare { dst, .. }
        | I::Convert { dst, .. }
        | I::IsNil { dst, .. }
        | I::IsPair { dst, .. }
        | I::IsArray { dst, .. }
        | I::IsArrayMut { dst, .. }
        | I::IsStruct { dst, .. }
        | I::IsStructMut { dst, .. }
        | I::IsSet { dst, .. }
        | I::IsSetMut { dst, .. }
        | I::ArrayMutLen { dst, .. }
        | I::MakeCaptureCell { dst, .. }
        | I::LoadCaptureCell { dst, .. }
        | I::MatchFail { dst, .. }
        | I::FirstDestructure { dst, .. }
        | I::RestDestructure { dst, .. }
        | I::ArrayMutRefDestructure { dst, .. }
        | I::ArrayMutSliceFrom { dst, .. }
        | I::StructGetOrNil { dst, .. }
        | I::StructGetDestructure { dst, .. }
        | I::StructRest { dst, .. }
        | I::FirstOrNil { dst, .. }
        | I::RestOrNil { dst, .. }
        | I::ArrayMutRefOrNil { dst, .. }
        | I::LoadResumeValue { dst }
        | I::Eval { dst, .. }
        | I::ArrayMutExtend { dst, .. }
        | I::ArrayMutPush { dst, .. }
        | I::IsEmpty { dst, .. }
        | I::IsBool { dst, .. }
        | I::IsInt { dst, .. }
        | I::IsFloat { dst, .. }
        | I::IsString { dst, .. }
        | I::IsKeyword { dst, .. }
        | I::IsSymbolCheck { dst, .. }
        | I::IsBytes { dst, .. }
        | I::IsBox { dst, .. }
        | I::IsClosure { dst, .. }
        | I::IsFiber { dst, .. }
        | I::TypeOf { dst, .. }
        | I::Length { dst, .. }
        | I::Get { dst, .. }
        | I::Put { dst, .. }
        | I::Del { dst, .. }
        | I::Has { dst, .. }
        | I::IntrPush { dst, .. }
        | I::IntrStringPush { dst, .. }
        | I::IntrBytesPush { dst, .. }
        | I::Pop { dst, .. }
        | I::Freeze { dst, .. }
        | I::Thaw { dst, .. }
        | I::Identical { dst, .. } => Some(dst),
        I::StoreLocal { .. }
        | I::StoreLocalRefcounted { .. }
        | I::StoreCapture { .. }
        | I::StoreCaptureCell { .. }
        | I::TailCall { .. }
        | I::TailCallArrayMut { .. }
        | I::IncrefRegion { .. }
        | I::DecrefRegion { .. }
        | I::DecrefValueRegion { .. }
        | I::DecrefCellRegion { .. }
        | I::IncrefValueRegion { .. }
        | I::AssertRegionMatches { .. }
        | I::AdoptRegion { .. }
        | I::AdoptCellRegion { .. }
        | I::AdoptIntoActivation { .. }
        | I::FreeRegionGroup { .. }
        | I::PushParamFrame { .. }
        | I::PopParamFrame
        | I::CheckSignalBound { .. } => None,
    }
}

/// The registers an instruction reads, once per operand position, in field
/// order.
pub(super) fn expected_uses(i: &InstrRef<'_>) -> Vec<Reg> {
    use InstrRef as I;
    match *i {
        I::Const { .. }
        | I::ValueConst { .. }
        | I::MaterializeConst { .. }
        | I::LoadLocal { .. }
        | I::LoadCapture { .. }
        | I::LoadCaptureRaw { .. }
        | I::LoadSelf { .. }
        | I::LoadResumeValue { .. }
        | I::IncrefRegion { .. }
        | I::DecrefRegion { .. }
        | I::PopParamFrame => vec![],
        I::StoreLocal { src, .. }
        | I::StoreLocalRefcounted { src, .. }
        | I::StoreCapture { src, .. }
        | I::CheckSignalBound { src, .. }
        | I::UnaryOp { src, .. }
        | I::Convert { src, .. }
        | I::IsNil { src, .. }
        | I::IsPair { src, .. }
        | I::IsArray { src, .. }
        | I::IsArrayMut { src, .. }
        | I::IsStruct { src, .. }
        | I::IsStructMut { src, .. }
        | I::IsSet { src, .. }
        | I::IsSetMut { src, .. }
        | I::ArrayMutLen { src, .. }
        | I::MatchFail { src, .. }
        | I::FirstDestructure { src, .. }
        | I::RestDestructure { src, .. }
        | I::ArrayMutRefDestructure { src, .. }
        | I::ArrayMutSliceFrom { src, .. }
        | I::StructGetOrNil { src, .. }
        | I::StructGetDestructure { src, .. }
        | I::StructRest { src, .. }
        | I::FirstOrNil { src, .. }
        | I::RestOrNil { src, .. }
        | I::ArrayMutRefOrNil { src, .. }
        | I::IsEmpty { src, .. }
        | I::IsBool { src, .. }
        | I::IsInt { src, .. }
        | I::IsFloat { src, .. }
        | I::IsString { src, .. }
        | I::IsKeyword { src, .. }
        | I::IsSymbolCheck { src, .. }
        | I::IsBytes { src, .. }
        | I::IsBox { src, .. }
        | I::IsClosure { src, .. }
        | I::IsFiber { src, .. }
        | I::TypeOf { src, .. }
        | I::Length { src, .. }
        | I::Pop { src, .. }
        | I::Freeze { src, .. }
        | I::Thaw { src, .. }
        | I::DecrefValueRegion { src }
        | I::DecrefCellRegion { src }
        | I::IncrefValueRegion { src }
        | I::AssertRegionMatches { src, .. } => vec![src],
        I::First { pair, .. } | I::Rest { pair, .. } => vec![pair],
        I::StoreCaptureCell { cell, value } => vec![cell, value],
        I::LoadCaptureCell { cell, .. } => vec![cell],
        I::MakeCaptureCell { value, .. } => vec![value],
        I::MakeClosure { captures, .. } => captures.to_vec(),
        I::Call { func, args, .. }
        | I::SuspendingCall { func, args, .. }
        | I::TailCall { func, args, .. } => {
            std::iter::once(func).chain(args.iter().copied()).collect()
        }
        I::CallArrayMut { func, args, .. } | I::TailCallArrayMut { func, args, .. } => {
            vec![func, args]
        }
        I::List { head, tail, .. } => vec![head, tail],
        I::MakeArrayMut { elements, .. } => elements.to_vec(),
        I::BinOp { lhs, rhs, .. } | I::Compare { lhs, rhs, .. } | I::Identical { lhs, rhs, .. } => {
            vec![lhs, rhs]
        }
        I::IntrPush { array, value, .. } => vec![array, value],
        I::IntrStringPush { string, value, .. } => vec![string, value],
        I::IntrBytesPush { bytes, value, .. } => vec![bytes, value],
        I::Get { obj, key, .. } | I::Del { obj, key, .. } | I::Has { obj, key, .. } => {
            vec![obj, key]
        }
        I::Put { obj, key, val, .. } => vec![obj, key, val],
        I::Eval { expr, env, .. } => vec![expr, env],
        I::ArrayMutExtend { array, source, .. } => vec![array, source],
        I::ArrayMutPush { array, value, .. } => vec![array, value],
        I::PushParamFrame { pairs } => pairs.to_vec(),
        I::AdoptRegion { parent, child } | I::AdoptCellRegion { parent, child } => {
            vec![parent, child]
        }
        I::AdoptIntoActivation { child } => vec![child],
        I::FreeRegionGroup { members } => members.to_vec(),
    }
}
