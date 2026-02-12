//! CPS-transformed expression type
//!
//! CpsExpr represents expressions after CPS transformation.
//! Pure expressions are preserved, while yielding expressions
//! are converted to continuation-passing style.

use super::Continuation;
use crate::compiler::ast::Expr;
use crate::value::{SymbolId, Value};
use std::rc::Rc;

/// A CPS-transformed expression
#[derive(Debug, Clone)]
pub enum CpsExpr {
    /// A pure expression that doesn't yield
    /// Evaluates expr, then applies the continuation to the result
    Pure {
        expr: Expr,
        continuation: Rc<Continuation>,
    },

    /// Yield a value and suspend
    Yield {
        /// Expression to evaluate and yield
        value: Box<CpsExpr>,
    },

    /// Call a function that may yield
    CpsCall {
        /// Function to call
        func: Box<CpsExpr>,
        /// Arguments
        args: Vec<CpsExpr>,
        /// Continuation after call returns
        continuation: Rc<Continuation>,
    },

    /// Call a pure function (won't yield)
    PureCall {
        /// Function to call
        func: Box<CpsExpr>,
        /// Arguments
        args: Vec<CpsExpr>,
        /// Continuation after call returns
        continuation: Rc<Continuation>,
    },

    /// Let binding
    Let {
        /// Variable being bound
        var: SymbolId,
        /// Initializer expression
        init: Box<CpsExpr>,
        /// Body expression
        body: Box<CpsExpr>,
    },

    /// Sequence of expressions
    Sequence {
        /// Expressions to evaluate
        exprs: Vec<CpsExpr>,
        /// Continuation after all expressions
        continuation: Rc<Continuation>,
    },

    /// Conditional
    If {
        /// Condition
        cond: Box<CpsExpr>,
        /// Then branch
        then_branch: Box<CpsExpr>,
        /// Else branch
        else_branch: Box<CpsExpr>,
        /// Continuation after chosen branch
        continuation: Rc<Continuation>,
    },

    /// While loop (may yield in body)
    While {
        /// Condition
        cond: Box<CpsExpr>,
        /// Body
        body: Box<CpsExpr>,
        /// Continuation after loop exits
        continuation: Rc<Continuation>,
    },

    /// For loop (may yield in body)
    For {
        /// Loop variable
        var: SymbolId,
        /// Iterator expression
        iter: Box<CpsExpr>,
        /// Body expression
        body: Box<CpsExpr>,
        /// Continuation after loop exits
        continuation: Rc<Continuation>,
    },

    /// And expression (short-circuit)
    And {
        /// Expressions to evaluate
        exprs: Vec<CpsExpr>,
        /// Continuation after evaluation
        continuation: Rc<Continuation>,
    },

    /// Or expression (short-circuit)
    Or {
        /// Expressions to evaluate
        exprs: Vec<CpsExpr>,
        /// Continuation after evaluation
        continuation: Rc<Continuation>,
    },

    /// Cond expression (multi-way conditional)
    Cond {
        /// Condition-body pairs
        clauses: Vec<(CpsExpr, CpsExpr)>,
        /// Optional else body
        else_body: Option<Box<CpsExpr>>,
        /// Continuation after chosen branch
        continuation: Rc<Continuation>,
    },

    /// Literal value
    Literal(Value),

    /// Variable reference
    Var {
        sym: SymbolId,
        depth: usize,
        index: usize,
    },

    /// Global variable reference
    GlobalVar(SymbolId),

    /// Lambda (closure creation)
    Lambda {
        params: Vec<SymbolId>,
        body: Box<CpsExpr>,
        captures: Vec<(SymbolId, usize, usize)>,
    },

    /// Return a value (for internal use)
    Return(Box<CpsExpr>),
}

impl CpsExpr {
    /// Check if this is a pure expression
    pub fn is_pure(&self) -> bool {
        matches!(
            self,
            CpsExpr::Pure { .. }
                | CpsExpr::Literal(_)
                | CpsExpr::Var { .. }
                | CpsExpr::GlobalVar(_)
        )
    }

    /// Check if this is a yield expression
    pub fn is_yield(&self) -> bool {
        matches!(self, CpsExpr::Yield { .. })
    }

    /// Create a literal CPS expression
    pub fn literal(value: Value) -> Self {
        CpsExpr::Literal(value)
    }

    /// Create a variable reference
    pub fn var(sym: SymbolId, depth: usize, index: usize) -> Self {
        CpsExpr::Var { sym, depth, index }
    }

    /// Create a global variable reference
    pub fn global_var(sym: SymbolId) -> Self {
        CpsExpr::GlobalVar(sym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_is_pure() {
        let expr = CpsExpr::literal(Value::Int(42));
        assert!(expr.is_pure());
    }

    #[test]
    fn test_yield_is_not_pure() {
        let expr = CpsExpr::Yield {
            value: Box::new(CpsExpr::literal(Value::Int(1))),
        };
        assert!(!expr.is_pure());
        assert!(expr.is_yield());
    }
}
