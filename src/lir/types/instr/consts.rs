// audited: 2026-09-06
// src/lir/AGENTS.md
//! The constants an instruction carries, visited in place.

use super::*;

impl LirInstr {
    /// Visit every `LirConst` this instruction carries.
    ///
    /// Exhaustive on purpose. The `send` path rewrites `LirConst::Symbol` ids
    /// into the loading process's table, and an instruction it fails to visit
    /// keeps the storing process's id — a silently wrong symbol, not an error.
    /// A new variant that carries a constant cannot be added without choosing
    /// its arm here.
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
