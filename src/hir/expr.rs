//! HIR expression types

use super::binding::{Binding, CaptureInfo};
use super::pattern::HirPattern;
use crate::effects::Effect;
use crate::syntax::Span;
use crate::value::{SymbolId, Value};

/// HIR expression with source location and effect
#[derive(Debug, Clone)]
pub struct Hir {
    pub kind: HirKind,
    pub span: Span,
    pub effect: Effect,
}

impl Hir {
    /// Create a new HIR node
    pub fn new(kind: HirKind, span: Span, effect: Effect) -> Self {
        Hir { kind, span, effect }
    }

    /// Create a pure HIR node
    pub fn pure(kind: HirKind, span: Span) -> Self {
        Hir {
            kind,
            span,
            effect: Effect::none(),
        }
    }
}

/// A function call argument, which may be spliced (spread).
#[derive(Debug, Clone)]
pub struct CallArg {
    pub expr: Hir,
    pub spliced: bool,
}

/// Unique identifier for a named/anonymous block, used by `break` to target
/// the correct block at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// HIR expression kinds - fully analyzed forms
#[derive(Debug, Clone)]
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
        /// If present, this function is variadic: extra args are collected
        /// into a list and bound to this parameter.
        rest_param: Option<Binding>,
        captures: Vec<CaptureInfo>,
        body: Box<Hir>,
        /// Number of local slots needed (params + locals)
        num_locals: u16,
        /// The inferred effect of CALLING this lambda.
        /// This may differ from body.effect for higher-order functions:
        /// - body.effect is the raw effect of the body expression
        /// - inferred_effect may be Polymorphic(i) if the Yields comes solely
        ///   from calling parameter i
        inferred_effect: Effect,
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
    /// Set! - mutate a binding
    Set {
        target: Binding,
        value: Box<Hir>,
    },

    /// Define - create/update a binding (global or local).
    /// The lowerer checks binding.is_global() to decide StoreGlobal vs StoreLocal.
    Define {
        binding: Binding,
        value: Box<Hir>,
    },

    // === Loops ===
    /// While loop
    While {
        cond: Box<Hir>,
        body: Box<Hir>,
    },

    /// For/each loop
    For {
        var: Binding,
        iter: Box<Hir>,
        body: Box<Hir>,
    },

    // === Pattern Matching ===
    Match {
        value: Box<Hir>,
        arms: Vec<(HirPattern, Option<Hir>, Hir)>, // pattern, guard, body
    },

    // === Short-circuit Boolean ===
    And(Vec<Hir>),
    Or(Vec<Hir>),

    // === Coroutines ===
    Yield(Box<Hir>),

    // === Quote ===
    /// Quote stores a pre-computed Value (converted at analysis time)
    Quote(Value),

    // === Destructuring ===
    /// Unconditional destructuring: extract values from a compound and bind them.
    /// Missing values → nil, no type checks, no branching on failure.
    /// Used by def/var/let/let*/fn when the binding position is a list or array.
    Destructure {
        pattern: HirPattern,
        value: Box<Hir>,
    },

    // === Module System ===
    Module {
        name: SymbolId,
        exports: Vec<SymbolId>,
        body: Box<Hir>,
    },

    Import {
        module: SymbolId,
    },

    /// Module-qualified reference
    ModuleRef {
        module: SymbolId,
        name: SymbolId,
    },

    /// Runtime eval: compile and execute a datum.
    /// `expr` evaluates to the value to compile.
    /// `env` evaluates to a table of name→value bindings (or nil for global-only).
    Eval {
        expr: Box<Hir>,
        env: Box<Hir>,
    },
}
