//! LIR type definitions

use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Number of closure-valued `ValueConst` instructions converted to
/// `ClosureRef` by `convert_value_consts_for_send` during the lifetime
/// of this process.
///
/// This path is exercised whenever user code references a stdlib
/// function (registered as a primitive via `update_cache_with_stdlib`)
/// from inside a closure that is sent across a `sys/spawn` boundary.
/// Exposed to Elle via the `lir/closure-value-const-count` primitive
/// and printed by `--stats`.
static CLOSURE_VALUE_CONST_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the lifetime count of closure-valued `ValueConst` instructions
/// serialized across `sys/spawn` boundaries. Reported by `--stats` and
/// exposed as an Elle primitive for regression tests.
pub fn closure_value_const_count() -> usize {
    CLOSURE_VALUE_CONST_COUNT.load(Ordering::Relaxed)
}

/// Virtual register
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub u32);

/// Index into an `LirModule`'s closure list.
///
/// `MakeClosure` references closures by ID rather than owning them,
/// so each closure is an independent compilation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// This closure's identity in the module's closure list.
    /// `None` for the entry function and standalone tests.
    pub closure_id: Option<ClosureId>,
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
    /// Bitmask indicating which parameters need to be wrapped in capture cells
    /// Bit i is set if parameter i needs a capture cell (for mutable parameters)
    pub capture_params_mask: u64,
    /// Bitmask indicating which locally-defined variables need capture cells.
    /// Bit i is set if locally-defined variable i needs a capture cell (captured or mutated).
    /// Variables without the bit set are stored directly without cell wrapping,
    /// avoiding heap allocation on every function call.
    pub capture_locals_mask: u64,
    /// Signal of this function (Pure, Yields, or Polymorphic)
    pub signal: Signal,
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
    /// Number of non-LBox parameters copied to local slots.
    /// These occupy the first `num_local_params` positions in `num_locals`.
    /// The `capture_locals_mask` indexes from position `num_local_params`.
    pub num_local_params: usize,
    /// Yield point metadata, populated during bytecode emission.
    /// Indexed by yield point order (0, 1, 2, ...).
    /// Empty for non-yielding functions.
    pub yield_points: Vec<YieldPointInfo>,
    /// Call site metadata, populated during bytecode emission.
    /// Only populated for functions where `signal.may_suspend()`.
    /// Indexed by call instruction order (0, 1, 2, ...).
    pub call_sites: Vec<CallSiteInfo>,
    /// True when the body's final expression is provably not a heap pointer.
    /// Used by fiber resume to decide whether shared allocation is needed.
    pub result_is_immediate: bool,
    /// True when the body contains `set!` to a captured binding with a
    /// potentially heap-allocated value. Used by fiber resume.
    pub has_outward_heap_set: bool,
    /// True when the function body is safe for tail-call pool rotation.
    pub rotation_safe: bool,
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
/// Only populated for functions where `signal.may_suspend()`.
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
            closure_id: None,
            name: None,
            arity,
            blocks: Vec::new(),
            entry: Label(0),
            constants: Vec::new(),
            num_regs: 0,
            num_locals: 0,
            num_captures: 0,
            capture_params_mask: 0,
            capture_locals_mask: 0,
            signal: Signal::silent(),
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params,
            num_local_params: 0,
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            result_is_immediate: false,
            has_outward_heap_set: false,
            rotation_safe: false,
        }
    }

    /// True if any block contains a SuspendingCall instruction.
    pub fn has_suspending_call(&self) -> bool {
        self.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|si| matches!(si.instr, LirInstr::SuspendingCall { .. }))
        })
    }

    /// True if this function is eligible for GPU compilation.
    ///
    /// GPU-eligible functions use only numeric operations (arithmetic,
    /// comparison, local variable access, control flow) with no heap
    /// allocation, closures, function calls, or signal emission.
    ///
    /// Checked in order of increasing cost:
    /// 1. Signal check (cheapest — just field reads)
    /// 2. Structural check (arity, captures, cells)
    /// 3. Instruction whitelist (walks all basic blocks)
    pub fn is_gpu_eligible(&self) -> bool {
        // Signal: allow error-only (arithmetic type errors can't happen on
        // unboxed GPU scalars), reject yield/IO/FFI/polymorphic
        let non_error = self.signal.bits.subtract(crate::signals::SIG_ERROR);
        if !non_error.is_empty() || self.signal.propagates != 0 {
            return false;
        }
        // Structural: no variadics, no mutable cells
        if !matches!(self.arity, Arity::Exact(_)) {
            return false;
        }
        if self.capture_params_mask != 0 || self.capture_locals_mask != 0 {
            return false;
        }
        // Instruction whitelist: every instruction and terminator must be GPU-safe
        self.blocks.iter().all(|b| {
            b.instructions
                .iter()
                .all(|si| is_gpu_instruction(&si.instr))
                && is_gpu_terminator(&b.terminator.terminator)
        })
    }

    /// True if this function is safe for the CPU MLIR tier-2 path.
    ///
    /// Stricter than `is_gpu_eligible`: the return register must be
    /// producible from numeric operations only. MLIR represents all
    /// values as i64, so nil (→ 0) can't round-trip back when the
    /// function is called from regular Elle code. Bool/Compare results
    /// are safe — the caller reboxes them as `Value::bool(result != 0)`.
    ///
    /// GPU dispatch (via `gpu:map`) doesn't have this problem — the
    /// caller reads integers out of a buffer and treats them as integers.
    pub fn is_mlir_cpu_eligible(&self) -> bool {
        if !self.is_gpu_eligible() {
            return false;
        }
        for block in &self.blocks {
            if let Terminator::Return(reg) = &block.terminator.terminator {
                if self.register_reaches_non_int(*reg) {
                    return false;
                }
            }
        }
        true
    }

    /// True if `target` is transitively produced by a non-numeric value
    /// source (Nil constant or IntToFloat conversion). Walks backward
    /// through definitions — Const sources, LoadLocal/StoreLocal chains.
    /// LoadCapture is treated as int (args are validated at call site).
    /// Bool constants and Compare results are i64 0/1 at the MLIR level;
    /// the caller reboxes as `Value::bool(result != 0)`.
    fn register_reaches_non_int(&self, target: Reg) -> bool {
        use std::collections::HashSet;
        let mut regs_to_check: Vec<Reg> = vec![target];
        let mut seen_regs: HashSet<u32> = HashSet::new();
        let mut seen_slots: HashSet<u16> = HashSet::new();
        while let Some(r) = regs_to_check.pop() {
            if !seen_regs.insert(r.0) {
                continue;
            }
            for block in &self.blocks {
                for si in &block.instructions {
                    match &si.instr {
                        LirInstr::Const {
                            dst,
                            value: LirConst::Nil,
                        } if *dst == r => return true,
                        LirInstr::Convert {
                            dst,
                            op: ConvOp::IntToFloat,
                            ..
                        } if *dst == r => return true,
                        // FloatToInt produces an int — safe, no action needed
                        LirInstr::LoadLocal { dst, slot }
                            if *dst == r && seen_slots.insert(*slot) =>
                        {
                            for b2 in &self.blocks {
                                for si2 in &b2.instructions {
                                    if let LirInstr::StoreLocal { slot: s, src } = &si2.instr {
                                        if *s == *slot {
                                            regs_to_check.push(*src);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        false
    }

    /// Convert ValueConst instructions to Const (LirConst) for safe cross-thread transfer.
    /// NativeFn ValueConsts are safe to keep as-is (function pointers are Send+Sync).
    /// Closure ValueConsts are converted to `ClosureRef(idx)` using the intern table.
    /// Returns false if any ValueConst contains a non-sendable, non-closure heap value.
    pub fn convert_value_consts_for_send(
        &mut self,
        visited: &std::collections::HashMap<u64, usize>,
    ) -> bool {
        for block in &mut self.blocks {
            for si in &mut block.instructions {
                if let LirInstr::ValueConst { dst, value } = &si.instr {
                    if value.is_native_fn() {
                        continue; // function pointers are thread-safe
                    }
                    let dst = *dst;
                    if let Some(lir_const) = value_to_lir_const(*value) {
                        si.instr = LirInstr::Const {
                            dst,
                            value: lir_const,
                        };
                    } else if value.is_closure() {
                        // Closure ValueConst: look up in intern table.
                        //
                        // This branch fires whenever a closure being sent
                        // across a `sys/spawn` boundary contains, in its
                        // LIR, a `ValueConst` holding a closure Value. That
                        // happens because stdlib functions are registered
                        // as primitives (see
                        // `src/primitives/module_init.rs::register_stdlib_exports`
                        // which calls `update_cache_with_stdlib`), so user
                        // code referencing a stdlib function inside a
                        // lambda lowers the reference to `ValueConst` via
                        // `immutable_values` in the lowerer. A spawned
                        // closure that transitively calls e.g. `inc` or
                        // `map` from stdlib will trip this branch.
                        //
                        // `CLOSURE_VALUE_CONST_COUNT` tracks the live count;
                        // see the `lir/closure-value-const-count` primitive
                        // and `--stats` output.
                        CLOSURE_VALUE_CONST_COUNT.fetch_add(1, Ordering::Relaxed);
                        if let Some(&idx) = visited.get(&value.payload) {
                            si.instr = LirInstr::Const {
                                dst,
                                value: LirConst::ClosureRef(idx),
                            };
                        } else {
                            return false;
                        }
                    } else {
                        // unsendable ValueConst (compound heap value)
                        return false;
                    }
                }
            }
        }
        true
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
    /// Create a closure. The closure body is in the module's closure
    /// list at the given `ClosureId`, not owned by this instruction.
    MakeClosure {
        dst: Reg,
        closure_id: ClosureId,
        captures: Vec<Reg>,
    },

    // === Function Calls ===
    /// Call a function (callee is known to not suspend).
    Call { dst: Reg, func: Reg, args: Vec<Reg> },
    /// Call a function that may suspend (yield, I/O, etc.).
    /// The WASM emitter creates a call-site continuation with spill/restore
    /// so the caller can resume if the callee yields.
    /// Only emitted inside functions whose signal includes may_suspend().
    SuspendingCall { dst: Reg, func: Reg, args: Vec<Reg> },
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
    /// Type conversion (float↔int intrinsics)
    Convert { dst: Reg, op: ConvOp, src: Reg },
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
    /// Check if value is an @array (for pattern matching)
    IsArrayMut { dst: Reg, src: Reg },
    /// Check if value is a struct (for pattern matching)
    IsStruct { dst: Reg, src: Reg },
    /// Check if value is an @struct (for pattern matching)
    IsStructMut { dst: Reg, src: Reg },
    /// Check if value is an immutable set (for pattern matching)
    IsSet { dst: Reg, src: Reg },
    /// Check if value is a mutable set (for pattern matching)
    IsSetMut { dst: Reg, src: Reg },
    /// Get array length (for pattern matching)
    ArrayMutLen { dst: Reg, src: Reg },

    // === Capture Cell Operations (for mutable captures) ===
    /// Create a capture cell containing a value
    MakeCaptureCell { dst: Reg, value: Reg },
    /// Load value from capture cell
    LoadCaptureCell { dst: Reg, cell: Reg },
    /// Store value into capture cell
    StoreCaptureCell { cell: Reg, value: Reg },

    // === Destructuring ===
    /// Car for destructuring: signals error if not a cons cell
    CarDestructure { dst: Reg, src: Reg },
    /// Cdr for destructuring: signals error if not a cons cell
    CdrDestructure { dst: Reg, src: Reg },
    /// Array ref for destructuring: signals error if out of bounds or not an array
    ArrayMutRefDestructure { dst: Reg, src: Reg, index: u16 },
    /// Array slice from index: returns a new array from index to end, or empty array
    ArrayMutSliceFrom { dst: Reg, src: Reg, index: u16 },
    /// Table/struct get with silent nil: nil if key missing/wrong type. Used by match.
    /// `key` is a constant pool index holding a keyword Value.
    StructGetOrNil { dst: Reg, src: Reg, key: LirConst },
    /// Table/struct get for destructuring: signals error if key missing or wrong type.
    /// `key` is a constant pool index holding a keyword Value.
    StructGetDestructure { dst: Reg, src: Reg, key: LirConst },

    /// Struct rest for destructuring: collect all keys from src NOT in exclude_keys
    /// into a new immutable struct. Used by `{:a a & rest}` patterns.
    /// `exclude_keys` are constant pool entries (keywords or symbols).
    StructRest {
        dst: Reg,
        src: Reg,
        exclude_keys: Vec<LirConst>,
    },

    // === Silent destructuring (parameter context: absent optional params → nil) ===
    /// Car with silent nil: returns nil if not a cons cell.
    /// Used for &opt/(required) parameter destructuring where absent values produce nil.
    CarOrNil { dst: Reg, src: Reg },
    /// Cdr with silent empty-list: returns EMPTY_LIST if not a cons cell.
    /// Used for &opt/(required) parameter destructuring.
    CdrOrNil { dst: Reg, src: Reg },
    /// Array ref with silent nil: returns nil if out of bounds or not an array.
    /// Used for `&opt`/\[required\] parameter destructuring.
    ArrayMutRefOrNil { dst: Reg, src: Reg, index: u16 },

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
    /// Extend an array with all elements of an indexed type (array or @array).
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
    /// Exit a call-scoped allocation region. Pops two scope marks:
    /// the barrier (top) and the region start (below). Frees only
    /// objects between the two marks (arg temporaries), leaving the
    /// callee's allocations intact.
    RegionExitCall,

    // === Dynamic Parameters ===
    /// Push a parameter frame. `pairs` contains (param_reg, value_reg) pairs.
    /// All param/value registers are consumed from the stack.
    PushParamFrame { pairs: Vec<(Reg, Reg)> },
    /// Pop the top parameter frame.
    /// No registers produced or consumed.
    PopParamFrame,

    // === Signal Checking ===
    /// Check that a closure's signal satisfies a bound.
    /// If the value in `src` is a closure whose `signal.bits & !allowed_bits != 0`,
    /// signal `:error`. Non-closures pass silently.
    /// If the check passes, execution continues.
    CheckSignalBound {
        src: Reg,
        allowed_bits: crate::value::fiber::SignalBits,
    },

    // === Outbox Routing ===
    /// Enter outbox routing context. Allocations between OutboxEnter and
    /// OutboxExit go to the fiber's outbox (for yield-bound values).
    /// No registers produced or consumed.
    OutboxEnter,
    /// Exit outbox routing context. Allocations revert to private heap.
    /// No registers produced or consumed.
    OutboxExit,

    // === Explicit rotation (Flip) ===
    /// Push a flip frame (save caller's swap pool; remember heap mark).
    /// No registers produced or consumed.
    FlipEnter,
    /// Rotate generations using the top flip frame's base. Tears down
    /// iteration N-2 and moves the current iteration into the swap pool.
    /// No registers produced or consumed.
    FlipSwap,
    /// Pop the top flip frame: tear down this frame's remaining swap pool
    /// and restore the caller's.
    /// No registers produced or consumed.
    FlipExit,
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

/// Conversion operations (type coercion intrinsics)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvOp {
    IntToFloat,
    FloatToInt,
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
    /// Emit a signal with compile-time signal bits and a runtime value.
    /// Execution resumes at resume_label with the resume value on the stack.
    /// Replaces the old `Yield` terminator; `(yield val)` becomes
    /// `Emit { signal: SIG_YIELD, ... }`.
    Emit {
        signal: crate::value::fiber::SignalBits,
        value: Reg,
        resume_label: Label,
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
    Keyword(String),
    /// Placeholder for a closure during cross-thread LIR transfer.
    /// The usize is the index into `SendBundle::closures`.
    /// Patched back to `ValueConst` during reconstruction.
    ClosureRef(usize),
}

/// Convert a runtime Value to a LirConst for safe cross-thread transfer.
/// Returns None for compound heap values (cons, arrays, closures, etc.)
/// that can't be represented as LirConst.
pub fn value_to_lir_const(v: Value) -> Option<LirConst> {
    if v.is_nil() {
        Some(LirConst::Nil)
    } else if v.is_empty_list() {
        Some(LirConst::EmptyList)
    } else if let Some(b) = v.as_bool() {
        Some(LirConst::Bool(b))
    } else if let Some(n) = v.as_int() {
        Some(LirConst::Int(n))
    } else if let Some(f) = v.as_float() {
        Some(LirConst::Float(f))
    } else if let Some(id) = v.as_symbol() {
        Some(LirConst::Symbol(SymbolId(id)))
    } else if let Some(name) = v.as_keyword_name() {
        Some(LirConst::Keyword(name))
    } else {
        v.with_string(|s| s.to_string()).map(LirConst::String)
    }
}

/// True if this LIR instruction is safe for GPU compilation.
///
/// GPU-safe: numeric constants, arithmetic, comparison, local/parameter
/// access. Everything else requires heap, closures, calls, or signals.
///
/// LoadCapture/LoadCaptureRaw are parameter or capture loads. Captures
/// are passed as extra parameters at the MLIR level.
fn is_gpu_instruction(i: &LirInstr) -> bool {
    matches!(
        i,
        LirInstr::Const {
            value: LirConst::Int(_) | LirConst::Float(_) | LirConst::Bool(_) | LirConst::Nil,
            ..
        } | LirInstr::BinOp { .. }
            | LirInstr::UnaryOp { .. }
            | LirInstr::Compare { .. }
            | LirInstr::Convert { .. }
            | LirInstr::LoadLocal { .. }
            | LirInstr::StoreLocal { .. }
            | LirInstr::LoadCapture { .. }
            | LirInstr::LoadCaptureRaw { .. }
    )
}

/// True if this block terminator is safe for GPU compilation.
///
/// GPU-safe: return, jump, branch. Emit (any signal) and Unreachable are not.
/// An Emit terminator means the function deliberately signals — even :error
/// via `(error ...)` is not GPU-safe.
fn is_gpu_terminator(t: &Terminator) -> bool {
    matches!(
        t,
        Terminator::Return(_) | Terminator::Jump(_) | Terminator::Branch { .. }
    )
}
