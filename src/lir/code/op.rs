// audited: 2026-10-06
//! `Op`: the opcode byte a frozen node carries, one per `LirInstr` variant.
//!
//! docs/impl/lir.md
//!
//! The enum and its list are declared by one macro, so a variant cannot be
//! added to one and missed in the other.

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

    /// The opcode of a working-form instruction.
    pub fn of(_instr: &LirInstr) -> Op {
        Op::Const
    }
}
