//! HIR expression types

use super::binding::{Binding, CaptureInfo};
use super::pattern::HirPattern;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::Value;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

mod op;
pub use op::*;

mod traverse;

/// Unique identifier for a HIR node. Used as a key for analysis side
/// tables (region assignments, type annotations, etc.).
///
/// Deliberately NOT `Ord`/`PartialOrd`: a `HirId` is an identity, not a
/// position. The global counter assigns ids monotonically, but the ANF
/// lift appends synthetic nodes whose ids do not reflect structural or
/// execution order, so comparing `HirId` magnitudes is meaningless and
/// was the source of a phantom-region class. Code that needs program
/// order must use the explicit index from
/// `crate::hir::liveness::compute_order` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HirId(pub u32);

/// Global monotonic counter for HirId assignment.
static NEXT_HIR_ID: AtomicU32 = AtomicU32::new(0);

/// Reset the HirId counter (call between compilation units).
pub fn reset_hir_ids() {
    NEXT_HIR_ID.store(0, Ordering::Relaxed);
}

fn fresh_hir_id() -> HirId {
    HirId(NEXT_HIR_ID.fetch_add(1, Ordering::Relaxed))
}

/// A declared signal bound on a function parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamBound {
    pub binding: Binding,
    pub signal: Signal,
}

/// HIR expression with source location, signal, and unique ID.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Hir {
    pub kind: HirKind,
    pub span: Span,
    pub signal: Signal,
    pub id: HirId,
}

impl Hir {
    /// Create a new HIR node with an auto-assigned unique ID.
    pub fn new(kind: HirKind, span: Span, signal: Signal) -> Self {
        Hir {
            kind,
            span,
            signal,
            id: fresh_hir_id(),
        }
    }

    /// Create a silent HIR node (no signals) with an auto-assigned ID.
    pub fn silent(kind: HirKind, span: Span) -> Self {
        Hir {
            kind,
            span,
            signal: Signal::silent(),
            id: fresh_hir_id(),
        }
    }
}

/// A function call argument, which may be spliced (spread).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallArg {
    pub expr: Hir,
    pub spliced: bool,
}

/// Unique identifier for a named/anonymous block, used by `break` to target
/// the correct block at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BlockId(pub u32);

/// How extra arguments beyond fixed params are collected.
/// Only meaningful when `rest_param` is `Some`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VarargKind {
    /// Collect into a list (existing `&` behavior)
    List,
    /// Collect into an immutable struct (`&keys`)
    Struct,
    /// Collect into an immutable struct (`&named`) with strict key validation.
    /// Contains the set of valid keyword names.
    StrictStruct(Vec<String>),
}

/// HIR expression kinds - fully analyzed forms
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HirKind {
    // === Literals ===
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Keyword(String),

    // === Variables ===
    /// Reference to a binding (fully resolved)
    Var(Binding),

    // === Binding Forms ===
    /// Let binding
    Let {
        bindings: Vec<(Binding, Hir)>,
        body: Box<Hir>,
    },

    /// Letrec (mutually recursive bindings)
    Letrec {
        bindings: Vec<(Binding, Hir)>,
        body: Box<Hir>,
    },

    /// Lambda expression
    Lambda {
        params: Vec<Binding>,
        /// Number of required parameters (before &opt).
        /// When no &opt, equals the count of fixed params
        /// (params.len() if no rest_param, params.len() - 1 if rest_param).
        num_required: usize,
        /// If present, this function is variadic: extra args are collected
        /// into a list or struct and bound to this parameter.
        rest_param: Option<Binding>,
        /// How the rest parameter's args are collected.
        /// Only meaningful when rest_param is Some.
        vararg_kind: VarargKind,
        captures: Vec<CaptureInfo>,
        body: Box<Hir>,
        /// Number of local slots needed (params + locals)
        num_locals: u16,
        /// The inferred signal of CALLING this lambda.
        /// This may differ from body.signal for higher-order functions:
        /// - body.signal is the raw signal of the body expression
        /// - inferred_signals may be Polymorphic(i) if the Yields comes solely
        ///   from calling parameter i
        /// - When `silence` bounds are present, bounded parameter calls contribute
        ///   their bound's bits directly (not polymorphic).
        inferred_signals: Signal,
        /// Declared signal bounds for parameters (from `(silence param)`).
        /// Only parameters with explicit bounds appear here.
        /// These bounds feed into inferred_signals computation and into runtime checking
        /// (`CheckSignalBound` for silence).
        param_bounds: Vec<ParamBound>,
        /// Optional docstring extracted from the lambda body. Plain compile-time
        /// string data (`Rc<str>`), NOT a heap `Value`. It rides the closure
        /// template (held alive by RC with it) and is materialized as a fresh
        /// ordinary (reclaimable) allocation when `(doc f)` reads it.
        #[serde(skip)]
        doc: Option<std::rc::Rc<str>>,
        /// Original lambda Syntax node for eval environment reconstruction
        #[serde(skip)]
        syntax: Option<Rc<crate::syntax::Syntax>>,
        /// True if the function body contains `(numeric!)` assertion.
        /// The lowerer checks `is_gpu_eligible()` after lowering.
        assert_numeric: bool,
    },

    // === Control Flow ===
    /// If expression
    If {
        cond: Box<Hir>,
        then_branch: Box<Hir>,
        else_branch: Box<Hir>,
    },

    /// Multi-way conditional
    Cond {
        clauses: Vec<(Hir, Hir)>,
        else_branch: Option<Box<Hir>>,
    },

    /// Sequence of expressions
    Begin(Vec<Hir>),

    /// Block with its own scope. May be named for targeted `break`.
    Block {
        name: Option<String>,
        block_id: BlockId,
        body: Vec<Hir>,
    },

    /// Early exit from a block, returning a value.
    Break {
        block_id: BlockId,
        value: Box<Hir>,
    },

    // === Function Application ===
    /// Function call
    Call {
        func: Box<Hir>,
        args: Vec<CallArg>,
        is_tail: bool,
    },

    // === Mutation ===
    /// Assign - mutate a var binding
    Assign {
        target: Binding,
        value: Box<Hir>,
    },

    /// Define - create/update a local binding.
    Define {
        binding: Binding,
        value: Box<Hir>,
    },

    // === Loops ===
    /// While loop (imperative — eliminated by functionalize pass)
    While {
        cond: Box<Hir>,
        body: Box<Hir>,
    },

    /// Functional loop with named bindings. Produced by the functionalize
    /// pass from While + Assign. `recur` jumps back to the top with new
    /// binding values.
    Loop {
        bindings: Vec<(Binding, Hir)>,
        body: Box<Hir>,
    },

    /// Jump back to the enclosing Loop with new values for its bindings.
    /// Must appear in tail position within a Loop body.
    Recur {
        args: Vec<Hir>,
    },

    // === Pattern Matching ===
    Match {
        value: Box<Hir>,
        arms: Vec<(HirPattern, Option<Hir>, Hir)>, // pattern, guard, body
    },

    // === Short-circuit Boolean ===
    And(Vec<Hir>),
    Or(Vec<Hir>),

    // === Signal emission ===
    /// `(emit <signal> <value>)` — general signal emission.
    /// `signal` is compile-time signal bits (from a literal keyword or set).
    /// `value` is the payload expression. Replaces the old `Yield` variant;
    /// `(yield val)` is now a macro expanding to `(emit :yield val)`.
    Emit {
        signal: crate::value::fiber::SignalBits,
        value: Box<Hir>,
    },

    // === Quote ===
    /// Quote of an IMMEDIATE datum (`'5`, `'foo`, `'()`) or a macro-hygiene
    /// datum carrying a pre-baked `SyntaxLiteral` Value: stores the Value
    /// directly (an immediate, or — only on the macro path — a syntax object).
    Quote(Value),
    /// Quote of COMPOUND DATA (`'(a b c)`, `'[1 2]`, nested structures): stores
    /// the immutable structure as a `ConstTemplate` — plain compile-time data,
    /// not a pre-baked pinned `Value`. `MaterializeConst` builds a fresh value
    /// from it into the literal's own region each execution (docs/impl/region/model.md
    /// § "Constants lower as ordinary allocations").
    QuoteConst(crate::value::ConstTemplate),

    // === Destructuring ===
    /// Unconditional destructuring: extract values from a compound and bind them.
    /// Used by def/var/let/let*/fn when the binding position is a list or array.
    /// `strict`: if true (binding forms: def/var/let/fn body), missing values signal error.
    /// `strict`: if false (parameter context: &opt, &keys patterns), missing values → nil.
    Destructure {
        pattern: HirPattern,
        value: Box<Hir>,
        strict: bool,
    },

    /// Runtime eval: compile and execute a datum.
    /// `expr` evaluates to the value to compile.
    /// `env` evaluates to a struct of name→value bindings (or nil for global-only).
    Eval {
        expr: Box<Hir>,
        env: Box<Hir>,
    },

    /// Dynamic parameter binding: `(parameterize ((p1 v1) (p2 v2) ...) body ...)`
    /// Pushes a parameter frame, evaluates body, pops the frame.
    /// Body is NOT in tail position (PopParamFrame must execute after).
    Parameterize {
        bindings: Vec<(Hir, Hir)>,
        body: Box<Hir>,
    },

    // === Cell operations (explicit CaptureCell) ===
    /// Wrap a value in a mutable cell (CaptureCell).
    /// Produced by functionalize for bindings that needs_capture().
    MakeCell {
        value: Box<Hir>,
    },

    /// Read the current value from a cell.
    DerefCell {
        cell: Box<Hir>,
    },

    /// Write a new value to a cell. Returns the written value.
    SetCell {
        cell: Box<Hir>,
        value: Box<Hir>,
    },

    /// Intrinsic operation: a %-prefixed special form that compiles
    /// directly to bytecode without function call overhead.
    Intrinsic {
        op: IntrinsicOp,
        args: Vec<Hir>,
    },

    /// Function-return ownership boundary. Wraps the tail value of a
    /// function body (one per non-tail-call tail leaf, inserted by
    /// `wrap_tail_returns` after `mark_tail_calls`). Evaluates `value`
    /// and hands the caller **one owning reference to the result's
    /// runtime region** — the callee side of the prediction-free
    /// calling convention. It is region-transparent: the result is the
    /// same value, living in the same region as `value`; the only
    /// effect is an incref of that region (lowered to
    /// `IncrefValueRegion`), balanced by the caller's release at the
    /// result binding's `decref_point`.
    ///
    /// This is **not** an early/control-flow return — Elle returns are
    /// implicit via tail position. It is a structural marker of "this
    /// expression's value escapes to the caller," distinct from a call.
    /// Tail calls are NOT wrapped: the inner callee already retained
    /// its result, and wrapping would defeat tail-call optimization.
    Return {
        value: Box<Hir>,
    },

    /// Poison node — inserted when a recoverable error is accumulated
    /// during analysis. The lowerer should never see this; the pipeline
    /// checks for accumulated errors before lowering.
    Error,
}

impl Hir {
    /// Create an error poison node (for error accumulation)
    pub fn error(span: Span) -> Self {
        Hir::silent(HirKind::Error, span)
    }

    /// Does evaluating this expression produce a freshly-allocated
    /// heap value owned by *this* HIR node?
    ///
    /// "Allocating" is **operational**, not syntactic: the predicate
    /// must agree with whether the lowerer at this `HirId` emits an
    /// instruction that increments a region's RC. Used by the ANF
    /// lift to decide which expressions need to be named.
    ///
    /// Notably:
    ///
    /// - `MakeCell` / `DerefCell` are **not** allocating. The lowerer
    ///   is transparent for them (the implicit `MakeCaptureCell` for
    ///   `needs_capture` bindings happens at the binding site, not
    ///   the `MakeCell` node). Wrapping their child in a synthetic
    ///   `Let` would manufacture a region with no matching alloc —
    ///   exactly the phantom-region class the region-inference audit
    ///   has been chasing.
    /// - `Eval` allocates because its runtime region is opaque to
    ///   the caller (the callee chooses), so the result needs a name
    ///   so `emit_decrefs_for` can emit `DecrefValueRegion`.
    /// - `Intrinsic` defers to `IntrinsicOp::allocates` (currently
    ///   `Pair`, `Freeze`, `Thaw`).
    /// - `Match` allocates iff any arm's pattern would allocate at
    ///   the destructure site (see `HirPattern::allocates`).
    pub fn allocates(&self) -> bool {
        match &self.kind {
            HirKind::Lambda { .. } => true,
            HirKind::Call { .. } => true,
            HirKind::Eval { .. } => true,
            // `produces_call_result_region`: %put/%del/%string-push/
            // %array-push/%bytes-push are call-results (region freed by value),
            // so ANF must NAME them to give the result a slot — otherwise the
            // `DecrefValueRegion` is orphaned when the value is consumed as an
            // operand or discarded.
            HirKind::Intrinsic { op, .. } => op.allocates() || op.produces_call_result_region(),
            HirKind::Match { arms, .. } => arms.iter().any(|(p, _, _)| p.allocates()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod allocates_tests;
