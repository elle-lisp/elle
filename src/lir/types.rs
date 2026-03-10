//! LIR type definitions

use crate::effects::Effect;
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};

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
    /// Bitmask indicating which locally-defined variables need cells.
    /// Bit i is set if locally-defined variable i needs a cell (captured or mutated).
    /// Variables without the bit set are stored directly without cell wrapping,
    /// avoiding heap allocation on every function call.
    pub cell_locals_mask: u64,
    /// Effect of this function (Pure, Yields, or Polymorphic)
    pub effect: Effect,
    /// Optional docstring from the source lambda
    pub doc: Option<Value>,
    /// Original lambda Syntax node for eval environment reconstruction
    pub syntax: Option<std::rc::Rc<crate::syntax::Syntax>>,
    /// How varargs are collected: List (cons chain) or Struct (immutable struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Total number of parameter slots (required + optional + rest if present).
    /// Used by VM populate_env to know how many fixed slots to fill.
    pub num_params: usize,
    /// Yield point metadata, populated during bytecode emission.
    /// Indexed by yield point order (0, 1, 2, ...).
    /// Empty for non-yielding functions.
    pub yield_points: Vec<YieldPointInfo>,
    /// Call site metadata, populated during bytecode emission.
    /// Only populated for functions where `effect.may_suspend()`.
    /// Indexed by call instruction order (0, 1, 2, ...).
    pub call_sites: Vec<CallSiteInfo>,
}

/// Metadata about a yield point, collected during bytecode emission.
/// The JIT reads this to know how to spill registers and where to
/// resume in the interpreter.
#[derive(Debug, Clone)]
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
/// Only populated for functions where `effect.may_suspend()`.
#[derive(Debug, Clone)]
pub struct CallSiteInfo {
    /// Bytecode IP after the Call instruction and its operands.
    /// This is the IP the interpreter would store in SuspendedFrame.ip
    /// when yield propagates through this call.
    pub resume_ip: usize,
    /// Registers on the operand stack at the call site, after popping
    /// func and args but before pushing the result. This matches the
    /// interpreter's stack state when yield propagates through a call
    /// (call_inner line 192: `self.fiber.stack.drain(..).collect()`).
    pub stack_regs: Vec<Reg>,
    /// Number of local variable slots (params + locally-defined).
    /// The interpreter stores locals at `[frame_base, frame_base + num_locals)`.
    /// The JIT must spill local values first, then operand stack registers,
    /// so the SuspendedFrame stack matches the interpreter's layout.
    pub num_locals: u16,
}

impl LirFunction {
    pub fn new(arity: Arity) -> Self {
        let num_params = arity.fixed_params();
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
            cell_locals_mask: 0,
            effect: Effect::inert(),
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params,
            yield_points: Vec::new(),
            call_sites: Vec::new(),
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
    MakeArrayMut { dst: Reg, elements: Vec<Reg> },
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
    /// Check if value is a tuple (for pattern matching)
    IsArray { dst: Reg, src: Reg },
    /// Check if value is an array (for pattern matching)
    IsArrayMut { dst: Reg, src: Reg },
    /// Check if value is a struct (for pattern matching)
    IsStruct { dst: Reg, src: Reg },
    /// Check if value is a table (for pattern matching)
    IsTable { dst: Reg, src: Reg },
    /// Check if value is an immutable set (for pattern matching)
    IsSet { dst: Reg, src: Reg },
    /// Check if value is a mutable set (for pattern matching)
    IsSetMut { dst: Reg, src: Reg },
    /// Get array length (for pattern matching)
    ArrayMutLen { dst: Reg, src: Reg },

    // === Cell Operations (for mutable captures) ===
    /// Create a cell containing a value
    MakeCell { dst: Reg, value: Reg },
    /// Load value from cell
    LoadCell { dst: Reg, cell: Reg },
    /// Store value into cell
    StoreCell { cell: Reg, value: Reg },

    // === Destructuring (silent nil) ===
    /// Car with silent nil: returns nil if not a cons cell
    CarOrNil { dst: Reg, src: Reg },
    /// Cdr with silent nil: returns nil if not a cons cell
    CdrOrNil { dst: Reg, src: Reg },
    /// Array/tuple ref with silent nil: returns nil if out of bounds or not an array/tuple
    ArrayMutRefOrNil { dst: Reg, src: Reg, index: u16 },
    /// Array/tuple slice from index: returns a new array from index to end, or empty array
    ArrayMutSliceFrom { dst: Reg, src: Reg, index: u16 },
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

    // === Splice Support ===
    /// Extend an array with all elements of an indexed type (array or tuple).
    /// Used by splice path: builds the args array incrementally.
    ArrayMutExtend { dst: Reg, array: Reg, source: Reg },
    /// Append a single value to an array.
    /// Used by splice path: adds non-spliced args to the args array.
    ArrayMutPush { dst: Reg, array: Reg, value: Reg },
    /// Call a function with elements of an array as arguments.
    /// The array is unpacked into individual arguments at runtime.
    CallArrayMut { dst: Reg, func: Reg, args: Reg },
    /// Tail call with elements of an array as arguments.
    TailCallArrayMut { func: Reg, args: Reg },

    // === Allocation Regions ===
    /// Enter an allocation region (scope boundary for allocator).
    /// No registers produced or consumed.
    RegionEnter,
    /// Exit an allocation region (scope boundary for allocator).
    /// No registers produced or consumed.
    RegionExit,

    // === Dynamic Parameters ===
    /// Push a parameter frame. `pairs` contains (param_reg, value_reg) pairs.
    /// All param/value registers are consumed from the stack.
    PushParamFrame { pairs: Vec<(Reg, Reg)> },
    /// Pop the top parameter frame.
    /// No registers produced or consumed.
    PopParamFrame,
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
