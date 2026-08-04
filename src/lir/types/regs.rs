//! The registers an instruction or terminator writes and reads.
//!
//! The single answer to that question for the whole crate: the WASM register
//! allocator and its liveness analysis walk these, and so does the test fixture
//! (`src/lir/testkit.rs`) when it infers a function's register count. The
//! matches are exhaustive, so a new `LirInstr` variant is reported here by the
//! compiler rather than silently treated as touching no register.

use super::{LirInstr, Reg, Terminator};

/// Calls `f` with each register `instr` writes.
///
/// A `TailCall`'s `dst` is not among them: the call replaces the frame, so the
/// register is written only on the JIT's native-callee completion path, which
/// no walker of this function serves.
pub fn for_each_def(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { dst, .. }
        | LirInstr::ValueConst { dst, .. }
        | LirInstr::MaterializeConst { dst, .. }
        | LirInstr::LoadLocal { dst, .. }
        | LirInstr::LoadCapture { dst, .. }
        | LirInstr::LoadCaptureRaw { dst, .. }
        | LirInstr::LoadSelf { dst, .. }
        | LirInstr::MakeClosure { dst, .. }
        | LirInstr::Call { dst, .. }
        | LirInstr::SuspendingCall { dst, .. }
        | LirInstr::CallArrayMut { dst, .. }
        | LirInstr::List { dst, .. }
        | LirInstr::MakeArrayMut { dst, .. }
        | LirInstr::First { dst, .. }
        | LirInstr::Rest { dst, .. }
        | LirInstr::BinOp { dst, .. }
        | LirInstr::UnaryOp { dst, .. }
        | LirInstr::Compare { dst, .. }
        | LirInstr::IsNil { dst, .. }
        | LirInstr::IsPair { dst, .. }
        | LirInstr::IsArray { dst, .. }
        | LirInstr::IsArrayMut { dst, .. }
        | LirInstr::IsStruct { dst, .. }
        | LirInstr::IsStructMut { dst, .. }
        | LirInstr::IsSet { dst, .. }
        | LirInstr::IsSetMut { dst, .. }
        | LirInstr::ArrayMutLen { dst, .. }
        | LirInstr::MakeCaptureCell { dst, .. }
        | LirInstr::LoadCaptureCell { dst, .. }
        | LirInstr::MatchFail { dst, .. }
        | LirInstr::FirstDestructure { dst, .. }
        | LirInstr::RestDestructure { dst, .. }
        | LirInstr::ArrayMutRefDestructure { dst, .. }
        | LirInstr::ArrayMutSliceFrom { dst, .. }
        | LirInstr::StructGetOrNil { dst, .. }
        | LirInstr::StructGetDestructure { dst, .. }
        | LirInstr::StructRest { dst, .. }
        | LirInstr::FirstOrNil { dst, .. }
        | LirInstr::RestOrNil { dst, .. }
        | LirInstr::ArrayMutRefOrNil { dst, .. }
        | LirInstr::LoadResumeValue { dst, .. }
        | LirInstr::Eval { dst, .. }
        | LirInstr::ArrayMutExtend { dst, .. }
        | LirInstr::ArrayMutPush { dst, .. }
        | LirInstr::Convert { dst, .. }
        | LirInstr::IsEmpty { dst, .. }
        | LirInstr::IsBool { dst, .. }
        | LirInstr::IsInt { dst, .. }
        | LirInstr::IsFloat { dst, .. }
        | LirInstr::IsString { dst, .. }
        | LirInstr::IsKeyword { dst, .. }
        | LirInstr::IsSymbolCheck { dst, .. }
        | LirInstr::IsBytes { dst, .. }
        | LirInstr::IsBox { dst, .. }
        | LirInstr::IsClosure { dst, .. }
        | LirInstr::IsFiber { dst, .. }
        | LirInstr::TypeOf { dst, .. }
        | LirInstr::Length { dst, .. }
        | LirInstr::Get { dst, .. }
        | LirInstr::Put { dst, .. }
        | LirInstr::Del { dst, .. }
        | LirInstr::Has { dst, .. }
        | LirInstr::Pop { dst, .. }
        | LirInstr::Freeze { dst, .. }
        | LirInstr::Thaw { dst, .. }
        | LirInstr::IntrPush { dst, .. }
        | LirInstr::IntrStringPush { dst, .. }
        | LirInstr::IntrBytesPush { dst, .. }
        | LirInstr::Identical { dst, .. } => f(*dst),

        LirInstr::StoreLocal { .. }
        | LirInstr::StoreLocalRefcounted { .. }
        | LirInstr::StoreCapture { .. }
        | LirInstr::StoreCaptureCell { .. }
        | LirInstr::TailCall { .. }
        | LirInstr::TailCallArrayMut { .. }
        | LirInstr::IncrefRegion { .. }
        | LirInstr::DecrefRegion { .. }
        | LirInstr::DecrefValueRegion { .. }
        | LirInstr::DecrefCellRegion { .. }
        | LirInstr::IncrefValueRegion { .. }
        | LirInstr::AssertRegionMatches { .. }
        | LirInstr::AdoptRegion { .. }
        | LirInstr::AdoptCellRegion { .. }
        | LirInstr::AdoptIntoActivation { .. }
        | LirInstr::FreeRegionGroup { .. }
        | LirInstr::PushParamFrame { .. }
        | LirInstr::PopParamFrame
        | LirInstr::CheckSignalBound { .. } => {}
    }
}

/// Calls `f` with each register `instr` reads, once per operand position.
pub fn for_each_use(instr: &LirInstr, mut f: impl FnMut(Reg)) {
    match instr {
        LirInstr::Const { .. }
        | LirInstr::ValueConst { .. }
        | LirInstr::MaterializeConst { .. } => {}
        LirInstr::LoadCapture { .. }
        | LirInstr::LoadCaptureRaw { .. }
        | LirInstr::LoadSelf { .. }
        | LirInstr::LoadResumeValue { .. } => {}

        LirInstr::LoadLocal { .. } => {}
        LirInstr::StoreLocal { src, .. } => f(*src),
        LirInstr::StoreCapture { src, .. } => f(*src),
        LirInstr::StoreCaptureCell { cell, value } => {
            f(*cell);
            f(*value);
        }
        LirInstr::CheckSignalBound { src, .. } => f(*src),

        LirInstr::MakeClosure { captures, .. } => {
            for c in captures {
                f(*c);
            }
        }

        LirInstr::Call { func, args, .. } | LirInstr::SuspendingCall { func, args, .. } => {
            f(*func);
            for a in args {
                f(*a);
            }
        }
        LirInstr::TailCall { func, args, .. } => {
            f(*func);
            for a in args {
                f(*a);
            }
        }
        LirInstr::CallArrayMut { func, args, .. } => {
            f(*func);
            f(*args);
        }
        LirInstr::TailCallArrayMut { func, args, .. } => {
            f(*func);
            f(*args);
        }

        LirInstr::List { head, tail, .. } => {
            f(*head);
            f(*tail);
        }
        LirInstr::MakeArrayMut { elements, .. } => {
            for e in elements {
                f(*e);
            }
        }
        LirInstr::First { pair, .. } | LirInstr::Rest { pair, .. } => f(*pair),

        LirInstr::BinOp { lhs, rhs, .. } | LirInstr::Compare { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }
        LirInstr::UnaryOp { src, .. }
        | LirInstr::IsNil { src, .. }
        | LirInstr::IsPair { src, .. }
        | LirInstr::IsArray { src, .. }
        | LirInstr::IsArrayMut { src, .. }
        | LirInstr::IsStruct { src, .. }
        | LirInstr::IsStructMut { src, .. }
        | LirInstr::IsSet { src, .. }
        | LirInstr::IsSetMut { src, .. }
        | LirInstr::ArrayMutLen { src, .. }
        | LirInstr::MatchFail { src, .. }
        | LirInstr::FirstDestructure { src, .. }
        | LirInstr::RestDestructure { src, .. }
        | LirInstr::ArrayMutRefDestructure { src, .. }
        | LirInstr::ArrayMutSliceFrom { src, .. }
        | LirInstr::StructGetOrNil { src, .. }
        | LirInstr::StructGetDestructure { src, .. }
        | LirInstr::StructRest { src, .. }
        | LirInstr::FirstOrNil { src, .. }
        | LirInstr::RestOrNil { src, .. }
        | LirInstr::ArrayMutRefOrNil { src, .. }
        | LirInstr::Convert { src, .. }
        | LirInstr::IsEmpty { src, .. }
        | LirInstr::IsBool { src, .. }
        | LirInstr::IsInt { src, .. }
        | LirInstr::IsFloat { src, .. }
        | LirInstr::IsString { src, .. }
        | LirInstr::IsKeyword { src, .. }
        | LirInstr::IsSymbolCheck { src, .. }
        | LirInstr::IsBytes { src, .. }
        | LirInstr::IsBox { src, .. }
        | LirInstr::IsClosure { src, .. }
        | LirInstr::IsFiber { src, .. }
        | LirInstr::TypeOf { src, .. }
        | LirInstr::Length { src, .. }
        | LirInstr::Pop { src, .. }
        | LirInstr::Freeze { src, .. }
        | LirInstr::Thaw { src, .. } => f(*src),

        LirInstr::IntrPush { array, value, .. } => {
            f(*array);
            f(*value);
        }
        LirInstr::IntrStringPush { string, value, .. } => {
            f(*string);
            f(*value);
        }
        LirInstr::IntrBytesPush { bytes, value, .. } => {
            f(*bytes);
            f(*value);
        }
        LirInstr::Get { obj, key, .. }
        | LirInstr::Del { obj, key, .. }
        | LirInstr::Has { obj, key, .. } => {
            f(*obj);
            f(*key);
        }
        LirInstr::Put { obj, key, val, .. } => {
            f(*obj);
            f(*key);
            f(*val);
        }
        LirInstr::Identical { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }

        LirInstr::MakeCaptureCell { value, .. } => f(*value),
        LirInstr::LoadCaptureCell { cell, .. } => f(*cell),

        LirInstr::Eval { expr, env, .. } => {
            f(*expr);
            f(*env);
        }
        LirInstr::ArrayMutExtend { array, source, .. } => {
            f(*array);
            f(*source);
        }
        LirInstr::ArrayMutPush { array, value, .. } => {
            f(*array);
            f(*value);
        }

        LirInstr::PushParamFrame { pairs } => {
            for (param, value) in pairs {
                f(*param);
                f(*value);
            }
        }

        LirInstr::IncrefRegion { .. } | LirInstr::DecrefRegion { .. } | LirInstr::PopParamFrame => {
        }

        LirInstr::StoreLocalRefcounted { src, .. } => f(*src),
        LirInstr::DecrefValueRegion { src, .. } => f(*src),
        LirInstr::DecrefCellRegion { src } => f(*src),
        LirInstr::IncrefValueRegion { src } => f(*src),
        // The oracle peeks `src` (the return value the slot is claimed to
        // name); record the use so liveness keeps it alive across the check.
        LirInstr::AssertRegionMatches { src, .. } => f(*src),
        // The ownership-forest ops load their operand values (the handler pops
        // them to drive the adopt / group free); record those uses so liveness
        // keeps them alive even though this backend never executes the op.
        LirInstr::AdoptRegion { parent, child } | LirInstr::AdoptCellRegion { parent, child } => {
            f(*parent);
            f(*child);
        }
        LirInstr::AdoptIntoActivation { child } => f(*child),
        LirInstr::FreeRegionGroup { members } => {
            for m in members {
                f(*m);
            }
        }
    }
}

/// Calls `f` with each register `term` reads: a returned value, a branch
/// condition, an emitted payload.
pub fn for_each_terminator_use(term: &Terminator, mut f: impl FnMut(Reg)) {
    match term {
        Terminator::Return(reg) => f(*reg),
        Terminator::Branch { cond, .. } => f(*cond),
        Terminator::Emit { value, .. } => f(*value),
        Terminator::Jump(_) | Terminator::Unreachable => {}
    }
}
