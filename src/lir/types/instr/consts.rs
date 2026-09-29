// audited: 2026-09-29
//! The constants an instruction carries, visited in place.
//!
//! src/lir/AGENTS.md

use super::*;

impl LirInstr {
    /// Visit every `LirConst` this instruction carries.
    ///
    /// Exhaustive on purpose: a variant that carries a constant cannot join the
    /// enum without choosing its arm here. The send tests walk it to read the
    /// `LirConst::Symbol` ids a deserialized function carries.
    pub fn for_each_const_mut(&mut self, mut f: impl FnMut(&mut LirConst)) {
        use LirInstr::*;
        match self {
            Const { value, .. } => f(value),
            StructGetOrNil { key, .. } => f(key),
            StructGetDestructure { key, .. } => f(key),
            StructRest { exclude_keys, .. } => exclude_keys.iter_mut().for_each(f),
            // Carries no `LirConst`.
            ValueConst { .. }
            | MaterializeConst { .. }
            | LoadLocal { .. }
            | StoreLocal { .. }
            | StoreLocalRefcounted { .. }
            | LoadCapture { .. }
            | LoadCaptureRaw { .. }
            | StoreCapture { .. }
            | MakeClosure { .. }
            | LoadSelf { .. }
            | Call { .. }
            | SuspendingCall { .. }
            | TailCall { .. }
            | List { .. }
            | MakeArrayMut { .. }
            | First { .. }
            | Rest { .. }
            | BinOp { .. }
            | UnaryOp { .. }
            | Convert { .. }
            | Compare { .. }
            | IsNil { .. }
            | IsPair { .. }
            | IsArray { .. }
            | IsArrayMut { .. }
            | IsStruct { .. }
            | IsStructMut { .. }
            | IsSet { .. }
            | IsSetMut { .. }
            | ArrayMutLen { .. }
            | MakeCaptureCell { .. }
            | LoadCaptureCell { .. }
            | StoreCaptureCell { .. }
            | MatchFail { .. }
            | FirstDestructure { .. }
            | RestDestructure { .. }
            | ArrayMutRefDestructure { .. }
            | ArrayMutSliceFrom { .. }
            | FirstOrNil { .. }
            | RestOrNil { .. }
            | ArrayMutRefOrNil { .. }
            | LoadResumeValue { .. }
            | Eval { .. }
            | ArrayMutExtend { .. }
            | ArrayMutPush { .. }
            | CallArrayMut { .. }
            | TailCallArrayMut { .. }
            | IncrefRegion { .. }
            | DecrefRegion { .. }
            | DecrefValueRegion { .. }
            | DecrefCellRegion { .. }
            | IncrefValueRegion { .. }
            | AdoptRegion { .. }
            | AdoptCellRegion { .. }
            | FreeRegionGroup { .. }
            | AdoptIntoActivation { .. }
            | AssertRegionMatches { .. }
            | JoinRegion { .. }
            | PushParamFrame { .. }
            | PopParamFrame
            | CheckSignalBound { .. }
            | IsEmpty { .. }
            | IsBool { .. }
            | IsInt { .. }
            | IsFloat { .. }
            | IsString { .. }
            | IsKeyword { .. }
            | IsSymbolCheck { .. }
            | IsBytes { .. }
            | IsBox { .. }
            | IsClosure { .. }
            | IsFiber { .. }
            | TypeOf { .. }
            | Length { .. }
            | Get { .. }
            | Put { .. }
            | Del { .. }
            | Has { .. }
            | IntrPush { .. }
            | IntrStringPush { .. }
            | IntrBytesPush { .. }
            | Pop { .. }
            | Freeze { .. }
            | Thaw { .. }
            | Identical { .. } => {}
        }
    }
}
