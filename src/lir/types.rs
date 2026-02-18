//! LIR type definitions

use crate::effects::Effect;
use crate::value::SymbolId;

/// Virtual register
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub u32);

impl Reg {
    pub fn new(id: u32) -> Self {
        Reg(id)
    }
}

/// Basic block label
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label(pub u32);

impl Label {
    pub fn new(id: u32) -> Self {
        Label(id)
    }
}

/// A LIR function (compilation unit)
#[derive(Debug, Clone)]
pub struct LirFunction {
    /// Function name (for debugging)
    pub name: Option<String>,
    /// Number of parameters
    pub arity: u16,
    /// Basic blocks
    pub blocks: Vec<BasicBlock>,
    /// Entry block label
    pub entry: Label,
    /// Constants used by this function
    pub constants: Vec<LirConst>,
    /// Number of registers used
    pub num_regs: u32,
    /// Number of local slots needed
    pub num_locals: u16,
    /// Bitmask indicating which parameters need to be wrapped in cells
    /// Bit i is set if parameter i needs a cell (for mutable parameters)
    pub cell_params_mask: u64,
    /// Effect of this function (Pure, Yields, or Polymorphic)
    pub effect: Effect,
}

impl LirFunction {
    pub fn new(arity: u16) -> Self {
        LirFunction {
            name: None,
            arity,
            blocks: Vec::new(),
            entry: Label(0),
            constants: Vec::new(),
            num_regs: 0,
            num_locals: 0,
            cell_params_mask: 0,
            effect: Effect::Pure,
        }
    }
}

/// A basic block - sequence of instructions ending in a terminator
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub label: Label,
    pub instructions: Vec<LirInstr>,
    pub terminator: Terminator,
}

impl BasicBlock {
    pub fn new(label: Label) -> Self {
        BasicBlock {
            label,
            instructions: Vec::new(),
            terminator: Terminator::Unreachable,
        }
    }
}

/// LIR instruction (SSA form - each register assigned exactly once)
#[derive(Debug, Clone)]
pub enum LirInstr {
    // === Constants ===
    /// Load a constant into a register
    Const { dst: Reg, value: LirConst },
    /// Load a Value constant into a register
    ValueConst {
        dst: Reg,
        value: crate::value::Value,
    },

    // === Variables ===
    /// Load from local slot
    LoadLocal { dst: Reg, slot: u16 },
    /// Store to local slot
    StoreLocal { slot: u16, src: Reg },
    /// Load from capture (auto-unwraps LocalCell)
    LoadCapture { dst: Reg, index: u16 },
    /// Load from capture without unwrapping (for forwarding cells to nested closures)
    LoadCaptureRaw { dst: Reg, index: u16 },
    /// Store to capture (handles cells automatically)
    StoreCapture { index: u16, src: Reg },
    /// Load global by symbol
    LoadGlobal { dst: Reg, sym: SymbolId },
    /// Store global by symbol
    StoreGlobal { sym: SymbolId, src: Reg },

    // === Closures ===
    /// Create a closure
    MakeClosure {
        dst: Reg,
        func: Box<LirFunction>,
        captures: Vec<Reg>,
    },

    // === Function Calls ===
    /// Call a function
    Call { dst: Reg, func: Reg, args: Vec<Reg> },
    /// Tail call (no return)
    TailCall { func: Reg, args: Vec<Reg> },

    // === Data Construction ===
    /// Construct a cons cell
    Cons { dst: Reg, head: Reg, tail: Reg },
    /// Construct a vector
    MakeVector { dst: Reg, elements: Vec<Reg> },
    /// Get car of cons
    Car { dst: Reg, pair: Reg },
    /// Get cdr of cons
    Cdr { dst: Reg, pair: Reg },

    // === Primitive Operations ===
    /// Binary arithmetic
    BinOp {
        dst: Reg,
        op: BinOp,
        lhs: Reg,
        rhs: Reg,
    },
    /// Unary operations
    UnaryOp { dst: Reg, op: UnaryOp, src: Reg },
    /// Comparison
    Compare {
        dst: Reg,
        op: CmpOp,
        lhs: Reg,
        rhs: Reg,
    },

    // === Type Checks ===
    /// Check if value is nil
    IsNil { dst: Reg, src: Reg },
    /// Check if value is a pair
    IsPair { dst: Reg, src: Reg },

    // === Cell Operations (for mutable captures) ===
    /// Create a cell containing a value
    MakeCell { dst: Reg, value: Reg },
    /// Load value from cell
    LoadCell { dst: Reg, cell: Reg },
    /// Store value into cell
    StoreCell { cell: Reg, value: Reg },

    // === Control Flow Helpers ===
    /// Copy a register (for phi-like operations)
    /// This is a logical copy - dst now refers to the same value as src.
    /// No bytecode is emitted; this just updates register tracking.
    Move { dst: Reg, src: Reg },
    /// Duplicate a register's value on the stack.
    /// Unlike Move, this actually emits a Dup instruction.
    /// Use this when you need both the original and a copy.
    Dup { dst: Reg, src: Reg },
    /// Pop a register's value from the stack (discard it).
    Pop { src: Reg },
    /// Inline conditional jump (for if expressions)
    JumpIfFalseInline { cond: Reg, label_id: u32 },
    /// Inline unconditional jump (for if expressions)
    JumpInline { label_id: u32 },
    /// Label marker for inline jumps (not emitted, just marks position)
    LabelMarker { label_id: u32 },

    // === Coroutines ===
    /// Yield a value
    Yield { dst: Reg, value: Reg },

    // === Exception Handling ===
    /// Push exception handler
    PushHandler { handler_label: Label },
    /// Pop exception handler
    PopHandler,
    /// Check if exception occurred
    CheckException,
    /// Match exception against handler exception ID (produces boolean result)
    MatchException { dst: Reg, exception_id: u16 },
    /// Bind caught exception to variable (by symbol name)
    BindException { var_name: SymbolId },
    /// Load current exception onto stack
    LoadException { dst: Reg },
    /// Clear current exception state
    ClearException,
    /// Re-raise: pop handler, reset handling flag, leave exception set
    ReraiseException,
    /// Throw exception
    Throw { value: Reg },
}

/// Binary operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

/// Comparison operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Block terminator - how control leaves a block
#[derive(Debug, Clone)]
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
    /// Unreachable (for incomplete blocks)
    Unreachable,
}

/// Constant values in LIR
#[derive(Debug, Clone)]
pub enum LirConst {
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Symbol(SymbolId),
    Keyword(SymbolId),
}
