//! LIR type definitions

use crate::effects::Effect;
use crate::syntax::Span;
use crate::value::{Arity, SymbolId};

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
    /// Function arity (Exact for fixed, AtLeast for variadic)
    pub arity: Arity,
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
    /// Number of captured variables
    /// Used by JIT to distinguish captures (from env) from parameters (from args)
    pub num_captures: u16,
    /// Bitmask indicating which parameters need to be wrapped in cells
    /// Bit i is set if parameter i needs a cell (for mutable parameters)
    pub cell_params_mask: u64,
    /// Effect of this function (Pure, Yields, or Polymorphic)
    pub effect: Effect,
}

impl LirFunction {
    pub fn new(arity: Arity) -> Self {
        LirFunction {
            name: None,
            arity,
            blocks: Vec::new(),
            entry: Label(0),
            constants: Vec::new(),
            num_regs: 0,
            num_locals: 0,
            num_captures: 0,
            cell_params_mask: 0,
            effect: Effect::none(),
        }
    }
}

/// An LIR instruction with source location
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
    /// Construct an array
    MakeArray { dst: Reg, elements: Vec<Reg> },
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
    /// Check if value is an array (for pattern matching)
    IsArray { dst: Reg, src: Reg },
    /// Check if value is a table or struct (for pattern matching)
    IsTable { dst: Reg, src: Reg },
    /// Get array length (for pattern matching)
    ArrayLen { dst: Reg, src: Reg },

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

    // === Destructuring (silent nil) ===
    /// Car with silent nil: returns nil if not a cons cell
    CarOrNil { dst: Reg, src: Reg },
    /// Cdr with silent nil: returns nil if not a cons cell
    CdrOrNil { dst: Reg, src: Reg },
    /// Array ref with silent nil: returns nil if out of bounds or not an array
    ArrayRefOrNil { dst: Reg, src: Reg, index: u16 },
    /// Array slice from index: returns a new array from index to end, or empty array
    ArraySliceFrom { dst: Reg, src: Reg, index: u16 },
    /// Table/struct get with silent nil: returns nil if key not found or wrong type.
    /// `key` is a constant pool index holding a keyword Value.
    TableGetOrNil { dst: Reg, src: Reg, key: LirConst },

    // === Coroutines ===
    /// Load the resume value after a yield.
    /// This is the first instruction in a yield's resume block.
    /// At runtime, the resume value is on top of the operand stack
    /// (pushed by the VM's resume_continuation).
    LoadResumeValue { dst: Reg },

    // === Runtime Eval ===
    /// Runtime eval: compile and execute a datum.
    /// Pops env and expr from stack, compiles and executes, pushes result.
    Eval { dst: Reg, expr: Reg, env: Reg },
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
    /// Yield control with a value. Execution resumes at resume_label
    /// with the resume value on the stack.
    Yield { value: Reg, resume_label: Label },
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
    Keyword(String),
}
