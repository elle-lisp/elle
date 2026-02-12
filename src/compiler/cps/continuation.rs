//! Continuation type - reified stack frames for coroutines

use crate::compiler::ast::Expr;
use crate::value::{SymbolId, Value};
use std::fmt;
use std::rc::Rc;

/// A reified continuation - captures "what to do next" after an expression completes
///
/// Continuations form a linked list representing the call stack.
/// When a coroutine yields, its continuation is saved and can be resumed later.
#[derive(Clone)]
pub enum Continuation {
    /// Top-level return - no more work to do
    Done,

    /// Continue evaluating a sequence of expressions (begin, block)
    /// After current expr, evaluate remaining exprs, then continue with `next`
    Sequence {
        /// Remaining expressions to evaluate
        remaining: Vec<Expr>,
        /// Continuation after all expressions are done
        next: Rc<Continuation>,
    },

    /// Continue after evaluating the condition in an if expression
    IfBranch {
        /// Then branch to evaluate if condition is truthy
        then_branch: Box<Expr>,
        /// Else branch to evaluate if condition is falsy
        else_branch: Box<Expr>,
        /// Continuation after the chosen branch completes
        next: Rc<Continuation>,
    },

    /// Continue after evaluating a let binding value
    LetBinding {
        /// Variable being bound
        var: SymbolId,
        /// Remaining bindings to evaluate
        remaining_bindings: Vec<(SymbolId, Expr)>,
        /// Values already bound
        bound_values: Vec<(SymbolId, Value)>,
        /// Body to evaluate after all bindings
        body: Box<Expr>,
        /// Continuation after body completes
        next: Rc<Continuation>,
    },

    /// Continue after a function call returns
    CallReturn {
        /// Environment to restore after call
        saved_env: Rc<Vec<Value>>,
        /// Continuation after processing return value
        next: Rc<Continuation>,
    },

    /// Apply a value to a continuation (for CPS-transformed code)
    Apply {
        /// The continuation function to apply
        cont_fn: Rc<dyn Fn(Value) -> crate::compiler::cps::Action + 'static>,
    },
}

impl fmt::Debug for Continuation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Continuation::Done => write!(f, "Continuation::Done"),
            Continuation::Sequence { remaining, next } => f
                .debug_struct("Continuation::Sequence")
                .field("remaining", remaining)
                .field("next", next)
                .finish(),
            Continuation::IfBranch {
                then_branch,
                else_branch,
                next,
            } => f
                .debug_struct("Continuation::IfBranch")
                .field("then_branch", then_branch)
                .field("else_branch", else_branch)
                .field("next", next)
                .finish(),
            Continuation::LetBinding {
                var,
                remaining_bindings,
                bound_values,
                body,
                next,
            } => f
                .debug_struct("Continuation::LetBinding")
                .field("var", var)
                .field("remaining_bindings", remaining_bindings)
                .field("bound_values", bound_values)
                .field("body", body)
                .field("next", next)
                .finish(),
            Continuation::CallReturn { saved_env, next } => f
                .debug_struct("Continuation::CallReturn")
                .field("saved_env", saved_env)
                .field("next", next)
                .finish(),
            Continuation::Apply { .. } => {
                write!(f, "Continuation::Apply {{ cont_fn: <function> }}")
            }
        }
    }
}

impl Continuation {
    /// Create a done continuation
    pub fn done() -> Rc<Self> {
        Rc::new(Continuation::Done)
    }

    /// Create a sequence continuation
    pub fn sequence(remaining: Vec<Expr>, next: Rc<Continuation>) -> Rc<Self> {
        if remaining.is_empty() {
            next
        } else {
            Rc::new(Continuation::Sequence { remaining, next })
        }
    }

    /// Create an if-branch continuation
    pub fn if_branch(then_branch: Expr, else_branch: Expr, next: Rc<Continuation>) -> Rc<Self> {
        Rc::new(Continuation::IfBranch {
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
            next,
        })
    }

    /// Check if this is the done continuation
    pub fn is_done(&self) -> bool {
        matches!(self, Continuation::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_done_continuation() {
        let cont = Continuation::done();
        assert!(cont.is_done());
    }

    #[test]
    fn test_sequence_empty() {
        let next = Continuation::done();
        let cont = Continuation::sequence(vec![], next.clone());
        // Empty sequence should just return next
        assert!(Rc::ptr_eq(&cont, &next));
    }

    #[test]
    fn test_sequence_non_empty() {
        let next = Continuation::done();
        let cont = Continuation::sequence(vec![Expr::Literal(Value::Int(1))], next);
        assert!(!cont.is_done());
    }
}
