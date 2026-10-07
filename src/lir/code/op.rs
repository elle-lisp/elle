// audited: 2026-10-06
//! `Op`: the opcode byte a frozen node carries, one per `LirInstr` variant.
//!
//! docs/impl/lir.md
//!
//! The enum and its list are declared by one macro, so a variant cannot be
//! added to one and missed in the other.

use super::instr::InstrRef;
use crate::lir::LirInstr;

macro_rules! ops {
    ($($name:ident),* $(,)?) => {
        /// One opcode per `LirInstr` variant, in declaration order. A variant
        /// means what the `LirInstr` variant of the same name means.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Op {
            $($name),*
        }

        impl Op {
            /// Every opcode, in byte order: `ALL[op as usize] == op`.
            pub const ALL: &'static [Op] = &[$(Op::$name),*];
        }
    };
}

ops!(
    Const,
    ValueConst,
    MaterializeConst,
    LoadLocal,
    StoreLocal,
    StoreLocalRefcounted,
    LoadCapture,
    LoadCaptureRaw,
    StoreCapture,
    MakeClosure,
    LoadSelf,
    Call,
    SuspendingCall,
    TailCall,
    List,
    MakeArrayMut,
    First,
    Rest,
    BinOp,
    UnaryOp,
    Convert,
    Compare,
    IsNil,
    IsPair,
    IsArray,
    IsArrayMut,
    IsStruct,
    IsStructMut,
    IsSet,
    IsSetMut,
    ArrayMutLen,
    MakeCaptureCell,
    LoadCaptureCell,
    StoreCaptureCell,
    MatchFail,
    FirstDestructure,
    RestDestructure,
    ArrayMutRefDestructure,
    ArrayMutSliceFrom,
    StructGetOrNil,
    StructGetDestructure,
    StructRest,
    FirstOrNil,
    RestOrNil,
    ArrayMutRefOrNil,
    LoadResumeValue,
    Eval,
    ArrayMutExtend,
    ArrayMutPush,
    CallArrayMut,
    TailCallArrayMut,
    IncrefRegion,
    DecrefRegion,
    DecrefValueRegion,
    DecrefCellRegion,
    IncrefValueRegion,
    AdoptRegion,
    AdoptCellRegion,
    FreeRegionGroup,
    AdoptIntoActivation,
    AssertRegionMatches,
    PushParamFrame,
    PopParamFrame,
    CheckSignalBound,
    IsEmpty,
    IsBool,
    IsInt,
    IsFloat,
    IsString,
    IsKeyword,
    IsSymbolCheck,
    IsBytes,
    IsBox,
    IsClosure,
    IsFiber,
    TypeOf,
    Length,
    Get,
    Put,
    Del,
    Has,
    IntrPush,
    IntrStringPush,
    IntrBytesPush,
    Pop,
    Freeze,
    Thaw,
    Identical,
);

impl Op {
    /// The opcode a stored byte names, or `None` for a byte past the last.
    pub fn from_byte(byte: u8) -> Option<Op> {
        Op::ALL.get(byte as usize).copied()
    }

    /// The opcode of an instruction. Exhaustive, so a new variant cannot be
    /// encoded until it has an opcode.
    pub fn of(instr: &InstrRef<'_>) -> Op {
        Op::of_working(&super::super::build::working_instr(*instr))
    }

    /// The opcode of a working-form instruction. Exhaustive, so a new variant
    /// has no frozen form until it has an opcode.
    pub fn of_working(instr: &LirInstr) -> Op {
        use LirInstr as I;
        match instr {
            I::Const { .. } => Op::Const,
            I::ValueConst { .. } => Op::ValueConst,
            I::MaterializeConst { .. } => Op::MaterializeConst,
            I::LoadLocal { .. } => Op::LoadLocal,
            I::StoreLocal { .. } => Op::StoreLocal,
            I::StoreLocalRefcounted { .. } => Op::StoreLocalRefcounted,
            I::LoadCapture { .. } => Op::LoadCapture,
            I::LoadCaptureRaw { .. } => Op::LoadCaptureRaw,
            I::StoreCapture { .. } => Op::StoreCapture,
            I::MakeClosure { .. } => Op::MakeClosure,
            I::LoadSelf { .. } => Op::LoadSelf,
            I::Call { .. } => Op::Call,
            I::SuspendingCall { .. } => Op::SuspendingCall,
            I::TailCall { .. } => Op::TailCall,
            I::List { .. } => Op::List,
            I::MakeArrayMut { .. } => Op::MakeArrayMut,
            I::First { .. } => Op::First,
            I::Rest { .. } => Op::Rest,
            I::BinOp { .. } => Op::BinOp,
            I::UnaryOp { .. } => Op::UnaryOp,
            I::Convert { .. } => Op::Convert,
            I::Compare { .. } => Op::Compare,
            I::IsNil { .. } => Op::IsNil,
            I::IsPair { .. } => Op::IsPair,
            I::IsArray { .. } => Op::IsArray,
            I::IsArrayMut { .. } => Op::IsArrayMut,
            I::IsStruct { .. } => Op::IsStruct,
            I::IsStructMut { .. } => Op::IsStructMut,
            I::IsSet { .. } => Op::IsSet,
            I::IsSetMut { .. } => Op::IsSetMut,
            I::ArrayMutLen { .. } => Op::ArrayMutLen,
            I::MakeCaptureCell { .. } => Op::MakeCaptureCell,
            I::LoadCaptureCell { .. } => Op::LoadCaptureCell,
            I::StoreCaptureCell { .. } => Op::StoreCaptureCell,
            I::MatchFail { .. } => Op::MatchFail,
            I::FirstDestructure { .. } => Op::FirstDestructure,
            I::RestDestructure { .. } => Op::RestDestructure,
            I::ArrayMutRefDestructure { .. } => Op::ArrayMutRefDestructure,
            I::ArrayMutSliceFrom { .. } => Op::ArrayMutSliceFrom,
            I::StructGetOrNil { .. } => Op::StructGetOrNil,
            I::StructGetDestructure { .. } => Op::StructGetDestructure,
            I::StructRest { .. } => Op::StructRest,
            I::FirstOrNil { .. } => Op::FirstOrNil,
            I::RestOrNil { .. } => Op::RestOrNil,
            I::ArrayMutRefOrNil { .. } => Op::ArrayMutRefOrNil,
            I::LoadResumeValue { .. } => Op::LoadResumeValue,
            I::Eval { .. } => Op::Eval,
            I::ArrayMutExtend { .. } => Op::ArrayMutExtend,
            I::ArrayMutPush { .. } => Op::ArrayMutPush,
            I::CallArrayMut { .. } => Op::CallArrayMut,
            I::TailCallArrayMut { .. } => Op::TailCallArrayMut,
            I::IncrefRegion { .. } => Op::IncrefRegion,
            I::DecrefRegion { .. } => Op::DecrefRegion,
            I::DecrefValueRegion { .. } => Op::DecrefValueRegion,
            I::DecrefCellRegion { .. } => Op::DecrefCellRegion,
            I::IncrefValueRegion { .. } => Op::IncrefValueRegion,
            I::AdoptRegion { .. } => Op::AdoptRegion,
            I::AdoptCellRegion { .. } => Op::AdoptCellRegion,
            I::FreeRegionGroup { .. } => Op::FreeRegionGroup,
            I::AdoptIntoActivation { .. } => Op::AdoptIntoActivation,
            I::AssertRegionMatches { .. } => Op::AssertRegionMatches,
            I::PushParamFrame { .. } => Op::PushParamFrame,
            I::PopParamFrame => Op::PopParamFrame,
            I::CheckSignalBound { .. } => Op::CheckSignalBound,
            I::IsEmpty { .. } => Op::IsEmpty,
            I::IsBool { .. } => Op::IsBool,
            I::IsInt { .. } => Op::IsInt,
            I::IsFloat { .. } => Op::IsFloat,
            I::IsString { .. } => Op::IsString,
            I::IsKeyword { .. } => Op::IsKeyword,
            I::IsSymbolCheck { .. } => Op::IsSymbolCheck,
            I::IsBytes { .. } => Op::IsBytes,
            I::IsBox { .. } => Op::IsBox,
            I::IsClosure { .. } => Op::IsClosure,
            I::IsFiber { .. } => Op::IsFiber,
            I::TypeOf { .. } => Op::TypeOf,
            I::Length { .. } => Op::Length,
            I::Get { .. } => Op::Get,
            I::Put { .. } => Op::Put,
            I::Del { .. } => Op::Del,
            I::Has { .. } => Op::Has,
            I::IntrPush { .. } => Op::IntrPush,
            I::IntrStringPush { .. } => Op::IntrStringPush,
            I::IntrBytesPush { .. } => Op::IntrBytesPush,
            I::Pop { .. } => Op::Pop,
            I::Freeze { .. } => Op::Freeze,
            I::Thaw { .. } => Op::Thaw,
            I::Identical { .. } => Op::Identical,
        }
    }
}
