// audited: 2026-10-06
//! The LIR's scalar types: registers, labels, operators, terminators, and the site records emission produces.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

/// Virtual register. `repr(transparent)`, so a frozen function's pool of `u32`
/// words reads as a slice of registers without a copy.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(transparent)]
pub struct Reg(pub u32);

/// Index into a compile unit's closure list.
///
/// `MakeClosure` references closures by ID rather than owning them,
/// so each closure is an independent compilation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ClosureId(pub u32);

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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// Calls `f` with each register `term` reads: a returned value, a branch
/// condition, an emitted payload. A node answers the same question for an
/// instruction (`NodeRef::uses`).
pub fn for_each_terminator_use(term: &Terminator, mut f: impl FnMut(Reg)) {
    match term {
        Terminator::Return(reg) => f(*reg),
        Terminator::Branch { cond, .. } => f(*cond),
        Terminator::Emit { value, .. } => f(*value),
        Terminator::Jump(_) | Terminator::Unreachable => {}
    }
}

/// Metadata about a yield point, collected during bytecode emission.
/// The JIT reads this to know how to spill registers and where to
/// resume in the interpreter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YieldPointInfo {
    /// Bytecode IP to resume at (the instruction after the Yield opcode).
    /// This is the IP stored in the SuspendedFrame so the interpreter
    /// can resume from the correct point.
    pub resume_ip: usize,
    /// Registers on the operand stack at the yield point, bottom-to-top.
    /// The JIT spills these Cranelift variables in this order to
    /// reconstruct the interpreter's operand stack on resume.
    pub stack_regs: Vec<Reg>,
    /// Number of local variable slots (params + locally-defined).
    /// The interpreter stores locals at `[frame_base, frame_base + num_locals)`.
    /// The JIT must spill local values first, then operand stack registers,
    /// so the SuspendedFrame stack matches the interpreter's layout.
    pub num_locals: u16,
}

/// Metadata about a call site, collected during bytecode emission.
/// The JIT reads this to know the bytecode IP at each call instruction,
/// which is needed to build SuspendedFrames for yield-through-call.
///
/// Only populated for functions where `signal.may_suspend()`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallSiteInfo {
    /// Bytecode IP after the Call instruction and its operands.
    /// This is the IP the interpreter would store in SuspendedFrame.ip
    /// when yield propagates through this call.
    pub resume_ip: usize,
    /// Registers on the operand stack at the call site, after popping
    /// func and args but before pushing the result. This matches the
    /// interpreter's stack state when yield propagates through a call
    /// (`complete_call` parks it with `self.fiber.stack.drain(..).collect()`).
    pub stack_regs: Vec<Reg>,
    /// Number of local variable slots (params + locally-defined).
    /// The interpreter stores locals at `[frame_base, frame_base + num_locals)`.
    /// The JIT must spill local values first, then operand stack registers,
    /// so the SuspendedFrame stack matches the interpreter's layout.
    pub num_locals: u16,
}
