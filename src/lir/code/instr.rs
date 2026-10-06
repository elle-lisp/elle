// audited: 2026-10-06
//! `InstrRef`: one frozen instruction as a reader matches it, over borrowed slices.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md
//!
//! Each variant means what the `LirInstr` variant of the same name means, and
//! carries the same fields: a slice where `LirInstr` holds a `Vec<Reg>`, a
//! `ConstRef` where it holds a `LirConst`, and a `TemplateBytes` where it holds
//! a `ConstTemplate`. The variant docs live on `LirInstr`.

use super::operand::{ConstList, ConstRef, Slots, TemplateBytes};
use crate::hir::region::StaticRegion;
use crate::lir::{BinOp, ClosureId, CmpOp, ConvOp, OperandProof, Reg, UnaryOp};
use crate::value::fiber::SignalBits;
use crate::value::{SymbolId, Value};

/// One frozen instruction. `LirInstr` documents each variant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InstrRef<'a> {
    Const {
        dst: Reg,
        value: ConstRef,
    },
    ValueConst {
        dst: Reg,
        value: Value,
    },
    MaterializeConst {
        dst: Reg,
        template: TemplateBytes<'a>,
        region: StaticRegion,
    },
    LoadLocal {
        dst: Reg,
        slot: u16,
    },
    StoreLocal {
        slot: u16,
        src: Reg,
    },
    StoreLocalRefcounted {
        slot: u16,
        src: Reg,
    },
    LoadCapture {
        dst: Reg,
        index: u16,
    },
    LoadCaptureRaw {
        dst: Reg,
        index: u16,
    },
    StoreCapture {
        index: u16,
        src: Reg,
    },
    MakeClosure {
        dst: Reg,
        closure_id: ClosureId,
        captures: &'a [Reg],
        region: StaticRegion,
    },
    LoadSelf {
        dst: Reg,
    },
    Call {
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
    },
    SuspendingCall {
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
    },
    TailCall {
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
        defer_callee_release: bool,
        deferred_release_slot: Option<StaticRegion>,
        borrowed_arg_slots: Slots<'a>,
    },
    List {
        dst: Reg,
        head: Reg,
        tail: Reg,
        region: StaticRegion,
    },
    MakeArrayMut {
        dst: Reg,
        elements: &'a [Reg],
        region: StaticRegion,
    },
    First {
        dst: Reg,
        pair: Reg,
    },
    Rest {
        dst: Reg,
        pair: Reg,
    },
    BinOp {
        dst: Reg,
        op: BinOp,
        lhs: Reg,
        rhs: Reg,
        proof: OperandProof,
    },
    UnaryOp {
        dst: Reg,
        op: UnaryOp,
        src: Reg,
        proof: OperandProof,
    },
    Convert {
        dst: Reg,
        op: ConvOp,
        src: Reg,
    },
    Compare {
        dst: Reg,
        op: CmpOp,
        lhs: Reg,
        rhs: Reg,
        proof: OperandProof,
    },
    IsNil {
        dst: Reg,
        src: Reg,
    },
    IsPair {
        dst: Reg,
        src: Reg,
    },
    IsArray {
        dst: Reg,
        src: Reg,
    },
    IsArrayMut {
        dst: Reg,
        src: Reg,
    },
    IsStruct {
        dst: Reg,
        src: Reg,
    },
    IsStructMut {
        dst: Reg,
        src: Reg,
    },
    IsSet {
        dst: Reg,
        src: Reg,
    },
    IsSetMut {
        dst: Reg,
        src: Reg,
    },
    ArrayMutLen {
        dst: Reg,
        src: Reg,
    },
    MakeCaptureCell {
        dst: Reg,
        value: Reg,
        region: StaticRegion,
        name: SymbolId,
        mutated: bool,
    },
    LoadCaptureCell {
        dst: Reg,
        cell: Reg,
    },
    StoreCaptureCell {
        cell: Reg,
        value: Reg,
    },
    MatchFail {
        dst: Reg,
        src: Reg,
    },
    FirstDestructure {
        dst: Reg,
        src: Reg,
    },
    RestDestructure {
        dst: Reg,
        src: Reg,
    },
    ArrayMutRefDestructure {
        dst: Reg,
        src: Reg,
        index: u16,
    },
    ArrayMutSliceFrom {
        dst: Reg,
        src: Reg,
        index: u16,
    },
    StructGetOrNil {
        dst: Reg,
        src: Reg,
        key: ConstRef,
    },
    StructGetDestructure {
        dst: Reg,
        src: Reg,
        key: ConstRef,
    },
    StructRest {
        dst: Reg,
        src: Reg,
        exclude_keys: ConstList<'a>,
    },
    FirstOrNil {
        dst: Reg,
        src: Reg,
    },
    RestOrNil {
        dst: Reg,
        src: Reg,
    },
    ArrayMutRefOrNil {
        dst: Reg,
        src: Reg,
        index: u16,
    },
    LoadResumeValue {
        dst: Reg,
    },
    Eval {
        dst: Reg,
        expr: Reg,
        env: Reg,
    },
    ArrayMutExtend {
        dst: Reg,
        array: Reg,
        source: Reg,
    },
    ArrayMutPush {
        dst: Reg,
        array: Reg,
        value: Reg,
    },
    CallArrayMut {
        dst: Reg,
        func: Reg,
        args: Reg,
        region: StaticRegion,
        args_region: StaticRegion,
    },
    TailCallArrayMut {
        func: Reg,
        args: Reg,
        region: StaticRegion,
        args_region: StaticRegion,
    },
    IncrefRegion {
        region_id: StaticRegion,
    },
    DecrefRegion {
        region_id: StaticRegion,
    },
    DecrefValueRegion {
        src: Reg,
    },
    DecrefCellRegion {
        src: Reg,
    },
    IncrefValueRegion {
        src: Reg,
    },
    AdoptRegion {
        parent: Reg,
        child: Reg,
    },
    AdoptCellRegion {
        parent: Reg,
        child: Reg,
    },
    FreeRegionGroup {
        members: &'a [Reg],
    },
    AdoptIntoActivation {
        child: Reg,
    },
    AssertRegionMatches {
        region_id: StaticRegion,
        src: Reg,
    },
    /// The (parameter, value) pairs, flat: parameter first.
    PushParamFrame {
        pairs: &'a [Reg],
    },
    PopParamFrame,
    CheckSignalBound {
        src: Reg,
        allowed_bits: SignalBits,
    },
    IsEmpty {
        dst: Reg,
        src: Reg,
    },
    IsBool {
        dst: Reg,
        src: Reg,
    },
    IsInt {
        dst: Reg,
        src: Reg,
    },
    IsFloat {
        dst: Reg,
        src: Reg,
    },
    IsString {
        dst: Reg,
        src: Reg,
    },
    IsKeyword {
        dst: Reg,
        src: Reg,
    },
    IsSymbolCheck {
        dst: Reg,
        src: Reg,
    },
    IsBytes {
        dst: Reg,
        src: Reg,
    },
    IsBox {
        dst: Reg,
        src: Reg,
    },
    IsClosure {
        dst: Reg,
        src: Reg,
    },
    IsFiber {
        dst: Reg,
        src: Reg,
    },
    TypeOf {
        dst: Reg,
        src: Reg,
    },
    Length {
        dst: Reg,
        src: Reg,
    },
    Get {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    Put {
        dst: Reg,
        obj: Reg,
        key: Reg,
        val: Reg,
    },
    Del {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    Has {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    IntrPush {
        dst: Reg,
        array: Reg,
        value: Reg,
    },
    IntrStringPush {
        dst: Reg,
        string: Reg,
        value: Reg,
    },
    IntrBytesPush {
        dst: Reg,
        bytes: Reg,
        value: Reg,
    },
    Pop {
        dst: Reg,
        src: Reg,
    },
    Freeze {
        dst: Reg,
        src: Reg,
        region: StaticRegion,
    },
    Thaw {
        dst: Reg,
        src: Reg,
        region: StaticRegion,
    },
    Identical {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
}

impl InstrRef<'_> {
    /// The static region slot this allocating or calling instruction is stamped
    /// with, as `LirInstr::region` answers it.
    pub fn region(&self) -> Option<StaticRegion> {
        match self {
            InstrRef::MakeClosure { region, .. }
            | InstrRef::Call { region, .. }
            | InstrRef::SuspendingCall { region, .. }
            | InstrRef::TailCall { region, .. }
            | InstrRef::List { region, .. }
            | InstrRef::MaterializeConst { region, .. }
            | InstrRef::MakeArrayMut { region, .. }
            | InstrRef::MakeCaptureCell { region, .. }
            | InstrRef::CallArrayMut { region, .. }
            | InstrRef::TailCallArrayMut { region, .. }
            | InstrRef::Freeze { region, .. }
            | InstrRef::Thaw { region, .. } => Some(*region),
            _ => None,
        }
    }
}
