use super::scope::ScopeType;
use crate::reader::SourceLoc;
use crate::value::{SymbolId, Value};

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
        match self.loc {
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

    /// Variable reference (symbol, depth, index)
    Var(SymbolId, usize, usize),

    /// Global variable reference
    GlobalVar(SymbolId),

    /// If expression
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },

    /// Cond expression (multi-way conditional)
    /// Evaluates each condition in order until one is true
    /// Each clause is (condition body...)
    Cond {
        clauses: Vec<(Expr, Expr)>,   // (condition, body) pairs
        else_body: Option<Box<Expr>>, // Optional else clause
    },

    /// Begin (sequence of expressions)
    Begin(Vec<Expr>),

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
        captures: Vec<(SymbolId, usize, usize)>,
    },

    /// Let binding
    Let {
        bindings: Vec<(SymbolId, Expr)>,
        body: Box<Expr>,
    },

    /// Set! (mutation)
    Set {
        var: SymbolId,
        depth: usize,
        index: usize,
        value: Box<Expr>,
    },

    /// Define (top-level only)
    Define { name: SymbolId, value: Box<Expr> },

    /// While loop
    While { cond: Box<Expr>, body: Box<Expr> },

    /// For loop (for element in list, execute body)
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
        catch: Option<(SymbolId, Box<Expr>)>, // variable name and handler
        finally: Option<Box<Expr>>,
    },

    /// Throw exception
    Throw { value: Box<Expr> },

    /// Quote expression (prevents evaluation)
    Quote(Box<Expr>),

    /// Quasiquote expression (quote with unquote support)
    Quasiquote(Box<Expr>),

    /// Unquote expression (inside quasiquote)
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

    /// Module-qualified name (e.g., math:add)
    ModuleRef { module: SymbolId, name: SymbolId },

    /// And operator (short-circuit evaluation)
    And(Vec<Expr>),

    /// Or operator (short-circuit evaluation)
    Or(Vec<Expr>),

    /// Xor operator (exclusive or, all args must be evaluated)
    Xor(Vec<Expr>),

    /// Scoped variable reference (depth, index)
    /// Used to reference variables that are in outer scopes
    ScopeVar(usize, usize),

    /// Scope entry (pushes a new scope onto the runtime scope stack)
    /// Emitted at the start of a scoped block
    ScopeEntry(ScopeType),

    /// Scope exit (pops the current scope from the runtime scope stack)
    /// Emitted at the end of a scoped block
    ScopeExit,
}

/// Pattern for pattern matching
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Match any value (wildcard)
    Wildcard,
    /// Match specific literal
    Literal(Value),
    /// Match variable and bind it
    Var(SymbolId),
    /// Match nil
    Nil,
    /// Match list with head and tail: (h . t)
    Cons {
        head: Box<Pattern>,
        tail: Box<Pattern>,
    },
    /// Match list with specific elements
    List(Vec<Pattern>),
    /// Guard pattern: pattern with condition
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
                | Expr::Let { .. }
                | Expr::While { .. }
                | Expr::For { .. }
                | Expr::Match { .. }
                | Expr::Try { .. }
                | Expr::And(_)
                | Expr::Or(_)
                | Expr::Xor(_)
        )
    }
}
