//! Continuation type - reified stack frames for coroutines

use crate::compiler::ast::Expr;
use crate::value::{SymbolId, Value};
use std::cell::RefCell;
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
        /// Environment to restore after call (shared mutable)
        saved_env: Rc<RefCell<Vec<Value>>>,
        /// Continuation after processing return value
        next: Rc<Continuation>,
    },

    /// Resume with saved environment
    /// Used to restore environment when resuming from yield
    WithEnv {
        /// Environment to restore
        env: Rc<RefCell<Vec<Value>>>,
        /// Inner continuation to apply
        inner: Rc<Continuation>,
    },

    /// Apply a value to a continuation (for CPS-transformed code)
    Apply {
        /// The continuation function to apply
        cont_fn: Rc<dyn Fn(Value) -> crate::compiler::cps::Action + 'static>,
    },

    /// Continue evaluating a CPS sequence after a yield
    /// This is used when a yield happens in the middle of a sequence
    CpsSequence {
        /// Remaining CPS expressions to evaluate
        remaining: Vec<crate::compiler::cps::CpsExpr>,
        /// Continuation after all expressions are done
        next: Rc<Continuation>,
    },

    /// Continue a while loop after a yield in the body
    CpsWhile {
        /// Condition expression
        cond: Box<crate::compiler::cps::CpsExpr>,
        /// Body expression
        body: Box<crate::compiler::cps::CpsExpr>,
        /// Continuation after loop exits
        next: Rc<Continuation>,
    },

    /// Continue the rest of a while loop body, then continue the loop
    /// This is used when a yield happens in the middle of the body
    CpsWhileBody {
        /// Continuation for the rest of the body
        body_cont: Rc<Continuation>,
        /// Condition expression for the loop
        cond: Box<crate::compiler::cps::CpsExpr>,
        /// Body expression for the loop
        body: Box<crate::compiler::cps::CpsExpr>,
        /// Continuation after loop exits
        next: Rc<Continuation>,
    },

    /// Continue with yield_cont first, then evaluate remaining expressions
    /// This is used when a yield happens inside a construct (like while) that's inside a sequence
    CpsSequenceAfterYield {
        /// Continuation from the yield (e.g., CpsWhile to continue a loop)
        yield_cont: Rc<Continuation>,
        /// Continuation for remaining expressions in the sequence
        remaining_cont: Rc<Continuation>,
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
            Continuation::CpsSequence { remaining, next } => f
                .debug_struct("Continuation::CpsSequence")
                .field("remaining_count", &remaining.len())
                .field("next", next)
                .finish(),
            Continuation::CpsWhile {
                cond: _,
                body: _,
                next,
            } => f
                .debug_struct("Continuation::CpsWhile")
                .field("next", next)
                .finish(),
            Continuation::CpsWhileBody {
                body_cont,
                cond: _,
                body: _,
                next,
            } => f
                .debug_struct("Continuation::CpsWhileBody")
                .field("body_cont", body_cont)
                .field("next", next)
                .finish(),
            Continuation::CpsSequenceAfterYield {
                yield_cont,
                remaining_cont,
            } => f
                .debug_struct("Continuation::CpsSequenceAfterYield")
                .field("yield_cont", yield_cont)
                .field("remaining_cont", remaining_cont)
                .finish(),
            Continuation::WithEnv { env, inner } => f
                .debug_struct("Continuation::WithEnv")
                .field("env_len", &env.borrow().len())
                .field("inner", inner)
                .finish(),
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
        let cont = Continuation::sequence(vec![Expr::Literal(Value::int(1))], next);
        assert!(!cont.is_done());
    }
}
