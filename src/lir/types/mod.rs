//! LIR type definitions

use crate::hir::region::StaticRegion;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

mod func;
mod instr;
mod regs;
pub use func::*;
pub use instr::*;
pub use regs::*;

/// Number of closure-valued `ValueConst` instructions converted to
/// `ClosureRef` by `convert_value_consts_for_send` during the lifetime
/// of this process.
///
/// This path is exercised whenever user code references a stdlib
/// function (registered as a primitive via `CompileCtx::register_stdlib_exports`)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Placeholder for a compound heap value (quoted list, struct, array, …)
    /// during cross-thread LIR transfer. The usize indexes the owning
    /// `SendableClosure::lir_value_pool`. Patched back to `ValueConst` during
    /// reconstruction — so, like `ClosureRef`, it never reaches the JIT/emitter.
    ValueRef(usize),
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
    match i {
        LirInstr::Const {
            value: LirConst::Int(_) | LirConst::Float(_) | LirConst::Bool(_) | LirConst::Nil,
            ..
        }
        | LirInstr::BinOp { .. }
        | LirInstr::UnaryOp { .. }
        | LirInstr::Compare { .. }
        | LirInstr::Convert { .. }
        | LirInstr::LoadLocal { .. }
        | LirInstr::StoreLocal { .. }
        | LirInstr::StoreLocalRefcounted { .. }
        | LirInstr::LoadCapture { .. }
        | LirInstr::LoadCaptureRaw { .. } => true,
        // Value-targeted region refcounts are no-ops on unboxed GPU
        // scalars (ints/floats carry no region) — the MLIR/SPIR-V
        // lowerers skip them. Every instruction that could put a heap
        // value in a register is rejected by this whitelist, so the
        // skipped refcounts can never unbalance a real region.
        LirInstr::IncrefValueRegion { .. } | LirInstr::DecrefValueRegion { .. } => true,
        // ValueConst of numeric/bool/nil types is GPU-safe — these are
        // immutable binding constants inlined by the lowerer.
        LirInstr::ValueConst { value, .. } => {
            value.is_int() || value.is_float() || value.as_bool().is_some() || value.is_nil()
        }
        // LoadSelf reads the executing-closure register — VM/JIT execution-context
        // state that has no meaning on an unboxed GPU scalar tier. Excluded
        // explicitly so a value-position self-reference is never GPU-dispatched.
        LirInstr::LoadSelf { .. } => false,
        // The activation adopt reaches the fiber's owner-node stack — VM/JIT
        // execution-context state with no meaning on the GPU tier. Excluded
        // explicitly so a function carrying it is never GPU-dispatched.
        LirInstr::AdoptIntoActivation { .. } => false,
        _ => false,
    }
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
