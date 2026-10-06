// audited: 2026-10-06
//! The LIR's types: a module, its functions' blocks and registers, and the operations and constants they hold.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

use crate::hir::region::StaticRegion;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::{Arity, SymbolId};

mod func;
mod instr;
mod regs;
pub use func::*;
pub use instr::*;
pub use regs::*;

/// Virtual register. `repr(transparent)`, so a frozen function's pool of `u32`
/// words reads as a slice of registers without a copy.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(transparent)]
pub struct Reg(pub u32);

/// Index into an `LirModule`'s closure list.
///
/// `MakeClosure` references closures by ID rather than owning them,
/// so each closure is an independent compilation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ClosureId(pub u32);

/// A module: an entry function plus independently compiled closures.
///
/// The entry function's `MakeClosure` instructions reference closures
/// by `ClosureId` (index into `closures`). Nested closures within
/// closures also reference by ID — the list is flat, depth-first.
#[derive(Debug, Clone)]
pub struct LirModule {
    pub entry: LirFunction,
    pub closures: Vec<LirFunction>,
}

impl Reg {
    pub fn new(id: u32) -> Self {
        Reg(id)
    }
}

/// Basic block label
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Label(pub u32);

impl Label {
    pub fn new(id: u32) -> Self {
        Label(id)
    }
}

/// An LIR instruction with source location.
///
/// There is no uniform `region` field: a region is carried by the *variants*
/// that need one (a mandatory `region: StaticRegion` field), and absent from
/// those that don't. "Region not applicable here" is encoded structurally by
/// the absence of the field — never by a sentinel 0 or an `Option` that every
/// instruction must drag along (which would let an allocation be built with no
/// region, the exact invalid state the newtype exists to forbid).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpannedInstr {
    pub instr: LirInstr,
    pub span: Span,
}

impl SpannedInstr {
    pub fn new(instr: LirInstr, span: Span) -> Self {
        SpannedInstr { instr, span }
    }
}

/// A terminator with source location
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpannedTerminator {
    pub terminator: Terminator,
    pub span: Span,
}

impl SpannedTerminator {
    pub fn new(terminator: Terminator, span: Span) -> Self {
        SpannedTerminator { terminator, span }
    }
}

/// A basic block - sequence of instructions ending in a terminator
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BasicBlock {
    pub label: Label,
    pub instructions: Vec<SpannedInstr>,
    pub terminator: SpannedTerminator,
}

impl BasicBlock {
    pub fn new(label: Label) -> Self {
        BasicBlock {
            label,
            instructions: Vec::new(),
            terminator: SpannedTerminator::new(Terminator::Unreachable, Span::synthetic()),
        }
    }
}

/// Binary operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// Unary operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

/// Conversion operations (type coercion intrinsics)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConvOp {
    IntToFloat,
    FloatToInt,
}

/// What the front end proved about an operation's operands.
///
/// The proof is discharged by the intrinsic operand contract
/// (`src/hir/typeinfer/contract.rs`) and carried here by the lowerer. A backend
/// spends it by dropping the tag test its generic path would run; see
/// docs/impl/lir.md for what each one does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum OperandProof {
    /// Nothing was proved. The operands can hold any value.
    #[default]
    Unproven,
    /// Every operand is an integer on every path that reaches the operation.
    Int,
}

impl OperandProof {
    /// Are the operands proven integers?
    pub fn is_int(self) -> bool {
        matches!(self, OperandProof::Int)
    }
}

/// Comparison operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Block terminator - how control leaves a block
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Terminator {
    /// Return from function
    Return(Reg),
    /// Unconditional jump
    Jump(Label),
    /// Conditional branch
    Branch {
        cond: Reg,
        then_label: Label,
        else_label: Label,
    },
    /// Emit a signal with compile-time signal bits and a runtime value.
    /// Execution resumes at resume_label with the resume value on the stack.
    /// `(yield val)` becomes `Emit { signal: SIG_YIELD, ... }`.
    Emit {
        signal: crate::value::fiber::SignalBits,
        value: Reg,
        resume_label: Label,
    },
    /// Unreachable (for incomplete blocks)
    Unreachable,
}

/// Constant values in LIR
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LirConst {
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// No producer emits it: a string literal lowers to `MaterializeConst`,
    /// and freezing refuses this variant by name.
    String(String),
    Symbol(SymbolId),
    Keyword(u64),
}
