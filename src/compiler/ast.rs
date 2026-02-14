use crate::binding::VarRef;
use crate::reader::SourceLoc;
use crate::value::{SymbolId, Value};

/// Information about a captured variable, including where to find it
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureInfo {
    pub sym: SymbolId,
    pub source: VarRef,
}

/// AST representation after macro expansion and analysis
#[derive(Debug, Clone, PartialEq)]
pub struct ExprWithLoc {
    pub expr: Expr,
    pub loc: Option<SourceLoc>,
}

impl ExprWithLoc {
    pub fn new(expr: Expr, loc: Option<SourceLoc>) -> Self {
        ExprWithLoc { expr, loc }
    }

    pub fn format_loc(&self) -> String {
        match &self.loc {
            Some(loc) => format!("{}:{}", loc.line, loc.col),
            None => "unknown".to_string(),
        }
    }
}

/// AST representation after macro expansion and analysis
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Literal value
    Literal(Value),

    /// Variable reference - fully resolved at compile time
    Var(VarRef),

    /// If expression
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },

    /// Cond expression (multi-way conditional)
    Cond {
        clauses: Vec<(Expr, Expr)>,
        else_body: Option<Box<Expr>>,
    },

    /// Begin (sequence of expressions)
    Begin(Vec<Expr>),

    /// Block expression (like begin but with its own scope)
    Block(Vec<Expr>),

    /// Function call
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        tail: bool,
    },

    /// Lambda expression
    Lambda {
        params: Vec<SymbolId>,
        body: Box<Expr>,
        captures: Vec<CaptureInfo>,
        num_locals: usize,
        /// Locally-defined variable symbols (from define statements in body)
        locals: Vec<SymbolId>,
    },

    /// Let binding
    Let {
        bindings: Vec<(SymbolId, Expr)>,
        body: Box<Expr>,
    },

    /// Letrec binding (mutually recursive bindings)
    Letrec {
        bindings: Vec<(SymbolId, Expr)>,
        body: Box<Expr>,
    },

    /// Set! (mutation) - target is a VarRef
    Set { target: VarRef, value: Box<Expr> },

    /// Define (top-level or local)
    Define { name: SymbolId, value: Box<Expr> },

    /// While loop
    While { cond: Box<Expr>, body: Box<Expr> },

    /// For loop
    For {
        var: SymbolId,
        iter: Box<Expr>,
        body: Box<Expr>,
    },

    /// Pattern matching expression
    Match {
        value: Box<Expr>,
        patterns: Vec<(Pattern, Expr)>,
        default: Option<Box<Expr>>,
    },

    /// Try-catch exception handling
    Try {
        body: Box<Expr>,
        catch: Option<(SymbolId, Box<Expr>)>,
        finally: Option<Box<Expr>>,
    },

    /// Throw exception
    Throw { value: Box<Expr> },

    /// Handler-case
    HandlerCase {
        body: Box<Expr>,
        handlers: Vec<(u32, SymbolId, Box<Expr>)>,
    },

    /// Handler-bind
    HandlerBind {
        handlers: Vec<(u32, Box<Expr>)>,
        body: Box<Expr>,
    },

    /// Quote expression
    Quote(Box<Expr>),

    /// Quasiquote expression
    Quasiquote(Box<Expr>),

    /// Unquote expression
    Unquote(Box<Expr>),

    /// Define macro
    DefMacro {
        name: SymbolId,
        params: Vec<SymbolId>,
        body: Box<Expr>,
    },

    /// Module definition
    Module {
        name: SymbolId,
        exports: Vec<SymbolId>,
        body: Box<Expr>,
    },

    /// Import module
    Import { module: SymbolId },

    /// Module-qualified name
    ModuleRef { module: SymbolId, name: SymbolId },

    /// And operator (short-circuit)
    And(Vec<Expr>),

    /// Or operator (short-circuit)
    Or(Vec<Expr>),

    /// Xor operator
    Xor(Vec<Expr>),

    /// Yield expression
    Yield(Box<Expr>),
}

/// Pattern for pattern matching
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Literal(Value),
    Var(SymbolId),
    Nil,
    Cons {
        head: Box<Pattern>,
        tail: Box<Pattern>,
    },
    List(Vec<Pattern>),
    Guard {
        pattern: Box<Pattern>,
        condition: Box<Expr>,
    },
}

impl Expr {
    pub fn is_tail_position(&self) -> bool {
        matches!(
            self,
            Expr::Call { tail: true, .. }
                | Expr::If { .. }
                | Expr::Cond { .. }
                | Expr::Begin(_)
                | Expr::Block(_)
                | Expr::Let { .. }
                | Expr::Letrec { .. }
                | Expr::While { .. }
                | Expr::For { .. }
                | Expr::Match { .. }
                | Expr::Try { .. }
                | Expr::HandlerCase { .. }
                | Expr::HandlerBind { .. }
                | Expr::And(_)
                | Expr::Or(_)
                | Expr::Xor(_)
        )
    }
}
